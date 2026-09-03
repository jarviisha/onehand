//! Main terminal view component for GPUI.
//!
//! This module provides [`TerminalView`], the primary component for embedding terminals
//! in GPUI applications. It manages:
//!
//! - **I/O Streams**: Accepts arbitrary [`Read`]/[`Write`]
//!   streams, allowing integration with any PTY implementation
//! - **Event Handling**: Keyboard and mouse input, with configurable callbacks
//! - **Rendering**: Efficient canvas-based rendering via [`TerminalRenderer`]
//! - **Configuration**: Font, colors, dimensions, and padding via [`TerminalConfig`]
//!
//! # Architecture
//!
//! The terminal uses a push-based async I/O architecture:
//!
//! 1. A background thread reads bytes from the PTY stdout in 4KB chunks
//! 2. Bytes are sent through a [flume](https://docs.rs/flume) channel to an async task
//! 3. The async task processes bytes through the VTE parser and calls `cx.notify()`
//! 4. GPUI repaints the terminal with the updated grid
//!
//! This approach ensures the terminal only wakes when data arrives, avoiding polling.
//!
//! *onehand patch*: the channel is **bounded** ([`READ_QUEUE_CHUNKS`]) and the
//! async task drains everything already queued into a single parse + repaint.
//! Upstream's unbounded channel let a fast writer outrun the parser without
//! limit, and one `notify` per 4KB chunk made that worse by repainting per
//! chunk.
//!
//! # Thread Safety
//!
//! - [`TerminalView`] itself is not `Send` (it contains GPUI handles)
//! - The stdin writer is wrapped in `Arc<parking_lot::Mutex<>>` for thread-safe writes
//! - Callbacks ([`ResizeCallback`], [`KeyHandler`]) must be `Send + Sync`
//!
//! # Example
//!
//! ```ignore
//! use gpui::{Context, Edges, px};
//! use gpui_terminal::{ColorPalette, TerminalConfig, TerminalView};
//!
//! // In a GPUI window context:
//! let terminal = cx.new(|cx| {
//!     TerminalView::new(pty_writer, pty_reader, TerminalConfig::default(), cx)
//!         .with_resize_callback(move |cols, rows| {
//!             // Notify PTY of new dimensions
//!         })
//!         .with_exit_callback(|_, cx| {
//!             cx.quit();
//!         })
//! });
//!
//! // Focus the terminal to receive keyboard input
//! terminal.read(cx).focus_handle().focus(window);
//! ```

use crate::colors::ColorPalette;
use crate::event::{GpuiEventProxy, TerminalEvent};
use crate::input::keystroke_to_bytes;
use crate::mouse::{button_report, encode_modifiers, motion_report, scroll_report};
use crate::render::TerminalRenderer;
use crate::terminal::TerminalState;
use alacritty_terminal::grid::Scroll;
use alacritty_terminal::index::{Column, Line, Point as AlacPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::TermMode;
use gpui::{Edges, *};
use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;

/// onehand patch: how many 4KB reads may sit between the PTY reader thread and
/// the parser.
///
/// Doubles as the batch cap on the consuming side, so one wake-up parses at
/// most this much before yielding — enough that a burst is absorbed in a frame
/// or two, small enough that the queue is bounded memory (~1MB) rather than
/// however much a runaway command can produce.
const READ_QUEUE_CHUNKS: usize = 256;

/// Configuration for terminal creation and runtime updates.
///
/// This struct defines the terminal's appearance and behavior, including
/// grid dimensions, font settings, scrollback buffer, and color scheme.
///
/// # Default Values
///
/// | Field | Default |
/// |-------|---------|
/// | `cols` | 80 |
/// | `rows` | 24 |
/// | `font_family` | "monospace" |
/// | `font_size` | 14px |
/// | `scrollback` | 10000 |
/// | `line_height_multiplier` | 1.0 |
/// | `padding` | 0px all sides |
/// | `colors` | Default palette |
///
/// # Example
///
/// ```ignore
/// use gpui::{Edges, px};
/// use gpui_terminal::{ColorPalette, TerminalConfig};
///
/// let config = TerminalConfig {
///     cols: 120,
///     rows: 40,
///     font_family: "JetBrains Mono".into(),
///     font_size: px(13.0),
///     scrollback: 50000,
///     line_height_multiplier: 1.0,
///     padding: Edges::all(px(10.0)),
///     colors: ColorPalette::builder()
///         .background(0x1a, 0x1a, 0x1a)
///         .foreground(0xe0, 0xe0, 0xe0)
///         .build(),
/// };
/// ```
///
/// # Runtime Updates
///
/// Configuration can be updated at runtime via [`TerminalView::update_config`].
/// This is useful for implementing features like dynamic font sizing:
///
/// ```ignore
/// terminal.update(cx, |terminal, cx| {
///     let mut config = terminal.config().clone();
///     config.font_size += px(1.0);
///     terminal.update_config(config, cx);
/// });
/// ```
#[derive(Clone, Debug)]
pub struct TerminalConfig {
    /// Number of columns (character width) in the terminal
    pub cols: usize,

    /// Number of rows (lines) in the terminal
    pub rows: usize,

    /// Font family name (e.g., "Fira Code", "JetBrains Mono")
    pub font_family: String,

    /// Font size in pixels
    pub font_size: Pixels,

    /// Maximum number of scrollback lines to keep in history
    pub scrollback: usize,

    /// Multiplier for line height to accommodate tall glyphs (e.g., nerd fonts)
    /// Default is 1.0 (no extra height)
    pub line_height_multiplier: f32,

    /// Padding around the terminal content (top, right, bottom, left)
    /// The padding area renders with the terminal's background color
    pub padding: Edges<Pixels>,

    /// Color palette for terminal colors (16 ANSI colors, 256 extended colors,
    /// foreground, background, and cursor colors)
    pub colors: ColorPalette,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            cols: 80,
            rows: 24,
            font_family: "monospace".into(),
            font_size: px(14.0),
            scrollback: 10000,
            line_height_multiplier: 1.0,
            padding: Edges::all(px(0.0)),
            colors: ColorPalette::default(),
        }
    }
}

/// Callback type for PTY resize notifications.
///
/// This callback is invoked when the terminal grid dimensions change,
/// typically due to window resizing. The callback receives the new
/// column and row counts.
///
/// # Arguments
///
/// * `cols` - New number of columns (characters wide)
/// * `rows` - New number of rows (lines tall)
///
/// # Thread Safety
///
/// This callback must be `Send + Sync` as it may be called from the render thread.
///
/// # Example
///
/// ```ignore
/// use portable_pty::PtySize;
///
/// let pty = Arc::new(Mutex::new(pty_master));
/// let pty_clone = pty.clone();
///
/// terminal.with_resize_callback(move |cols, rows| {
///     pty_clone.lock().resize(PtySize {
///         cols: cols as u16,
///         rows: rows as u16,
///         pixel_width: 0,
///         pixel_height: 0,
///     }).ok();
/// });
/// ```
pub type ResizeCallback = Box<dyn Fn(usize, usize) + Send + Sync>;

/// Callback type for key event interception.
///
/// This callback is invoked before the terminal processes a key event,
/// allowing you to intercept and handle specific key combinations.
///
/// # Arguments
///
/// * `event` - The key down event from GPUI
///
/// # Returns
///
/// * `true` - Consume the event (terminal will not process it)
/// * `false` - Let the terminal handle the event normally
///
/// # Thread Safety
///
/// This callback must be `Send + Sync`.
///
/// # Example
///
/// ```ignore
/// terminal.with_key_handler(|event| {
///     let keystroke = &event.keystroke;
///
///     // Intercept Ctrl++ for font size increase
///     if keystroke.modifiers.control && (keystroke.key == "+" || keystroke.key == "=") {
///         // Handle font size increase
///         return true; // Consume the event
///     }
///
///     // Intercept Ctrl+- for font size decrease
///     if keystroke.modifiers.control && keystroke.key == "-" {
///         // Handle font size decrease
///         return true;
///     }
///
///     false // Let terminal handle all other keys
/// });
/// ```
pub type KeyHandler = Box<dyn Fn(&KeyDownEvent) -> bool + Send + Sync>;

/// Callback for terminal bell events.
///
/// This callback is invoked when the terminal bell is triggered (BEL character,
/// ASCII 0x07), allowing you to play a sound or show a visual indicator.
///
/// # Arguments
///
/// * `window` - The GPUI window
/// * `cx` - The context for the TerminalView
///
/// # Example
///
/// ```ignore
/// terminal.with_bell_callback(|window, cx| {
///     // Option 1: Visual bell (flash the window or show an indicator)
///     // Option 2: Play a sound
///     // Option 3: Notify the user via system notification
/// });
/// ```
pub type BellCallback = Box<dyn Fn(&mut Window, &mut Context<TerminalView>)>;

/// Callback for terminal title changes.
///
/// This callback is invoked when the terminal title changes via escape sequences
/// (OSC 0, OSC 2), allowing you to update the window or tab title.
///
/// # Arguments
///
/// * `window` - The GPUI window
/// * `cx` - The context for the TerminalView
/// * `title` - The new title string
///
/// # Example
///
/// ```ignore
/// terminal.with_title_callback(|window, cx, title| {
///     // Update the window title
///     // Or update a tab label in a tabbed interface
///     println!("Terminal title changed to: {}", title);
/// });
/// ```
pub type TitleCallback = Box<dyn Fn(&mut Window, &mut Context<TerminalView>, &str)>;

/// Callback for clipboard store requests.
///
/// This callback is invoked when the terminal wants to store data to the clipboard
/// via OSC 52 escape sequence. Applications like tmux and vim can use this to
/// copy text to the system clipboard.
///
/// # Arguments
///
/// * `window` - The GPUI window
/// * `cx` - The context for the TerminalView
/// * `text` - The text to store in the clipboard
///
/// # Example
///
/// ```ignore
/// use gpui_terminal::Clipboard;
///
/// terminal.with_clipboard_store_callback(|window, cx, text| {
///     if let Ok(mut clipboard) = Clipboard::new() {
///         clipboard.copy(text).ok();
///     }
/// });
/// ```
pub type ClipboardStoreCallback = Box<dyn Fn(&mut Window, &mut Context<TerminalView>, &str)>;

/// Callback for terminal exit events.
///
/// This callback is invoked when the terminal process exits (e.g., shell exits,
/// process terminates). This is detected when the PTY reader reaches EOF.
///
/// # Arguments
///
/// * `window` - The GPUI window
/// * `cx` - The context for the TerminalView
///
/// # Example
///
/// ```ignore
/// terminal.with_exit_callback(|window, cx| {
///     // Option 1: Quit the application
///     cx.quit();
///
///     // Option 2: Close this terminal tab/pane
///     // terminal_manager.close_terminal(terminal_id);
///
///     // Option 3: Show an exit message
///     // show_notification("Terminal exited");
/// });
/// ```
pub type ExitCallback = Box<dyn Fn(&mut Window, &mut Context<TerminalView>)>;

/// The main terminal view component for GPUI applications.
///
/// `TerminalView` is a GPUI entity that implements the [`Render`] trait,
/// providing a complete terminal emulator that can be embedded in any GPUI application.
///
/// # Responsibilities
///
/// - **Terminal State**: Manages the grid, cursor, and colors via [`TerminalState`]
/// - **I/O Streams**: Reads from PTY stdout and writes to PTY stdin
/// - **Event Handling**: Processes keyboard, mouse, and resize events
/// - **Rendering**: Paints text, backgrounds, and cursor via [`TerminalRenderer`]
/// - **Callbacks**: Dispatches events to user-provided callbacks
///
/// # Creating a Terminal
///
/// Use [`TerminalView::new`] within a GPUI entity context:
///
/// ```ignore
/// let terminal = cx.new(|cx| {
///     TerminalView::new(writer, reader, config, cx)
///         .with_resize_callback(resize_callback)
///         .with_exit_callback(|_, cx| cx.quit())
/// });
/// ```
///
/// # Focus
///
/// The terminal must be focused to receive keyboard input:
///
/// ```ignore
/// terminal.read(cx).focus_handle().focus(window);
/// ```
///
/// # Callbacks
///
/// Configure behavior through builder methods:
///
/// - [`with_resize_callback`](Self::with_resize_callback) - PTY size changes
/// - [`with_exit_callback`](Self::with_exit_callback) - Process exit
/// - [`with_key_handler`](Self::with_key_handler) - Key event interception
/// - [`with_bell_callback`](Self::with_bell_callback) - Terminal bell
/// - [`with_title_callback`](Self::with_title_callback) - Title changes
/// - [`with_clipboard_store_callback`](Self::with_clipboard_store_callback) - Clipboard writes
///
/// # Thread Safety
///
/// `TerminalView` is not `Send` as it contains GPUI handles. The stdin writer
/// is internally wrapped in `Arc<parking_lot::Mutex<>>` for safe concurrent access.
pub struct TerminalView {
    /// The terminal state managing the grid and VTE parser
    state: TerminalState,

    /// The renderer for drawing terminal content
    renderer: TerminalRenderer,

    /// Focus handle for keyboard event handling
    focus_handle: FocusHandle,

    /// Writer for sending input to the terminal process
    stdin_writer: Arc<parking_lot::Mutex<Box<dyn Write + Send>>>,

    /// Receiver for terminal events from the event proxy
    event_rx: mpsc::Receiver<TerminalEvent>,

    /// Configuration used to create this terminal
    config: TerminalConfig,

    /// Async task that reads bytes and notifies the view (push-based)
    #[allow(dead_code)]
    _reader_task: Task<()>,

    /// Callback to notify the PTY about size changes
    resize_callback: Option<Arc<ResizeCallback>>,

    /// Optional callback to intercept key events before terminal processing
    key_handler: Option<Arc<KeyHandler>>,

    /// Callback for terminal bell events
    bell_callback: Option<BellCallback>,

    /// Callback for terminal title changes
    title_callback: Option<TitleCallback>,

    /// Callback for clipboard store requests
    clipboard_store_callback: Option<ClipboardStoreCallback>,

    /// Callback for terminal exit events
    exit_callback: Option<ExitCallback>,

    // ── onehand patch: the interaction layer ────────────────────────────────
    //
    // Upstream ships a render core with `on_mouse_up`, `on_mouse_move`,
    // `on_scroll` and the clipboard left as empty TODOs, and never imports its
    // own `mouse.rs`. Everything below is what makes those work.
    /// The content rect of the last paint, in window coordinates.
    ///
    /// Mouse events arrive in window space and the grid lives in cell space;
    /// this is the only place that conversion can come from, and it is only
    /// known after a paint.
    content_origin: Point<Pixels>,
    /// True while the left button is held after a press inside the grid.
    ///
    /// Tracked rather than read from the event: a drag that leaves the widget
    /// must keep extending the selection, so "is the pointer inside" is the
    /// wrong question once a drag has started.
    dragging: bool,
    /// The button whose press was reported to the child, if any.
    ///
    /// Not read from the move event's own `pressed_button`, which answers a
    /// different question: this one is "did *we* hand the press over", and it is
    /// what decides whether the drag that follows is the child's gesture or a
    /// selection. A press that started a selection leaves this `None` even
    /// though a button is down.
    reported_button: Option<MouseButton>,
    /// The cell the last motion report named.
    ///
    /// Motion arrives per pixel and a report is per cell, so without this a
    /// pointer moved slowly across one character sends the same coordinates
    /// dozens of times to a program that redraws on each.
    reported_cell: Option<AlacPoint>,
    /// The input method's in-progress composition, if any.
    ///
    /// Held by the view rather than written into the grid because it is not
    /// input yet: the child process must not see a syllable until the input
    /// method says it is finished. Cleared by the commit that ends it.
    preedit: String,
    /// Where the composition was last drawn, in window coordinates.
    ///
    /// The platform asks for this to place the candidate list, and it can only
    /// be answered from a paint -- the cursor's cell is a grid coordinate until
    /// the content rect is known.
    preedit_bounds: Bounds<Pixels>,
}

impl TerminalView {
    /// Create a new terminal with provided I/O streams.
    ///
    /// This method initializes a new terminal emulator with the given stdin writer
    /// and stdout reader. It spawns a background task to read from stdout and
    /// process incoming bytes through the VTE parser.
    ///
    /// # Arguments
    ///
    /// * `stdin_writer` - Writer for sending input bytes to the terminal process
    /// * `stdout_reader` - Reader for receiving output bytes from the terminal process
    /// * `config` - Terminal configuration (dimensions, font, etc.)
    /// * `cx` - GPUI context for this view
    ///
    /// # Returns
    ///
    /// A new `TerminalView` instance ready to be rendered.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // In a GPUI window context:
    /// let terminal = cx.new(|cx| {
    ///     TerminalView::new(stdin_writer, stdout_reader, TerminalConfig::default(), cx)
    /// });
    /// ```
    pub fn new<W, R>(
        stdin_writer: W,
        stdout_reader: R,
        config: TerminalConfig,
        cx: &mut Context<Self>,
    ) -> Self
    where
        W: Write + Send + 'static,
        R: Read + Send + 'static,
    {
        // Create event channel for terminal events
        let (event_tx, event_rx) = mpsc::channel();

        // Clone event_tx for the reader task to send Exit event when PTY closes
        let exit_event_tx = event_tx.clone();

        // Create event proxy for alacritty
        let event_proxy = GpuiEventProxy::new(event_tx);

        // Create terminal state
        let state = TerminalState::new(config.cols, config.rows, event_proxy);

        // Create renderer with font settings and color palette
        let renderer = TerminalRenderer::new(
            config.font_family.clone(),
            config.font_size,
            config.line_height_multiplier,
            config.colors.clone(),
        );

        // Create focus handle
        let focus_handle = cx.focus_handle();

        // Wrap stdin writer in Arc<Mutex> for thread-safe access
        let stdin_writer = Arc::new(parking_lot::Mutex::new(
            Box::new(stdin_writer) as Box<dyn Write + Send>
        ));

        // Create async channel for bytes (push-based notification)
        // Using flume instead of smol::channel because flume is executor-agnostic
        // and properly wakes GPUI's async executor when data arrives
        //
        // onehand patch: bounded, not unbounded. The reader thread is never
        // blocked by the PTY, so with an unbounded queue a command that writes
        // faster than the parser can consume (`yes`, `cat` of a large file, a
        // noisy build log) grows this queue without limit -- and the scrollback
        // cap sits *after* the parser, so it bounds nothing here. A bounded
        // channel makes the reader block instead, which is the backpressure a
        // real terminal already relies on: the writer slows down.
        let (bytes_tx, bytes_rx) = flume::bounded::<Vec<u8>>(READ_QUEUE_CHUNKS);

        // Spawn background thread to read from stdout
        // This thread sends bytes through the async channel
        thread::spawn(move || {
            Self::read_stdout_blocking(stdout_reader, bytes_tx);
        });

        // Spawn async task that awaits on the channel and notifies the view
        // This is push-based: the task blocks until bytes arrive, then immediately notifies
        let reader_task = cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            loop {
                // Wait for bytes from the background reader (blocks until data arrives)
                match bytes_rx.recv_async().await {
                    Ok(bytes) => {
                        // onehand patch: drain whatever else is already queued
                        // and parse it in the same update. One `notify` per 4KB
                        // chunk is one repaint per 4KB of output, which is how
                        // the UI fell behind a fast writer in the first place;
                        // the grid only has to be correct once per frame.
                        let mut batch = vec![bytes];
                        while let Ok(more) = bytes_rx.try_recv() {
                            batch.push(more);
                            if batch.len() >= READ_QUEUE_CHUNKS {
                                break;
                            }
                        }
                        // Process bytes and notify the view
                        let result = this.update(cx, |view: &mut Self, cx: &mut Context<Self>| {
                            for bytes in &batch {
                                view.state.process_bytes(bytes);
                            }
                            cx.notify();
                        });
                        if result.is_err() {
                            // View was dropped, exit
                            break;
                        }
                    }
                    Err(_) => {
                        // Channel closed - PTY has finished, send Exit event
                        let _ = exit_event_tx.send(TerminalEvent::Exit);
                        // Notify view to process the Exit event
                        let _ = this.update(cx, |_view, cx: &mut Context<Self>| {
                            cx.notify();
                        });
                        break;
                    }
                }
            }
        });

        Self {
            state,
            renderer,
            focus_handle,
            stdin_writer,
            event_rx,
            config,
            _reader_task: reader_task,
            resize_callback: None,
            key_handler: None,
            bell_callback: None,
            title_callback: None,
            clipboard_store_callback: None,
            exit_callback: None,
            content_origin: Point::default(),
            dragging: false,
            reported_button: None,
            reported_cell: None,
            preedit: String::new(),
            preedit_bounds: Bounds::default(),
        }
    }

    /// Set a callback to be invoked when the terminal is resized.
    ///
    /// This callback should resize the underlying PTY to match the new dimensions.
    /// The callback receives (cols, rows) as arguments.
    ///
    /// # Arguments
    ///
    /// * `callback` - A function that will be called with (cols, rows) on resize
    pub fn with_resize_callback(
        mut self,
        callback: impl Fn(usize, usize) + Send + Sync + 'static,
    ) -> Self {
        self.resize_callback = Some(Arc::new(Box::new(callback)));
        self
    }

    /// Set a callback to intercept key events before terminal processing.
    ///
    /// The callback receives the key event and should return `true` to consume
    /// the event (prevent the terminal from processing it), or `false` to allow
    /// normal terminal processing.
    ///
    /// # Arguments
    ///
    /// * `handler` - A function that receives key events and returns whether to consume them
    ///
    /// # Example
    ///
    /// ```ignore
    /// terminal.with_key_handler(|event| {
    ///     // Handle Ctrl++ to increase font size
    ///     if event.keystroke.modifiers.control && event.keystroke.key == "+" {
    ///         // Handle the event
    ///         return true; // Consume the event
    ///     }
    ///     false // Let terminal handle it
    /// })
    /// ```
    pub fn with_key_handler(
        mut self,
        handler: impl Fn(&KeyDownEvent) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.key_handler = Some(Arc::new(Box::new(handler)));
        self
    }

    /// Set a callback to be invoked when the terminal bell is triggered.
    ///
    /// The callback receives a mutable reference to the window and context,
    /// allowing you to play a sound or show a visual indicator.
    ///
    /// # Arguments
    ///
    /// * `callback` - A function that will be called when the bell is triggered
    ///
    /// # Example
    ///
    /// ```ignore
    /// terminal.with_bell_callback(|window, cx| {
    ///     // Play a sound or flash the screen
    /// })
    /// ```
    pub fn with_bell_callback(
        mut self,
        callback: impl Fn(&mut Window, &mut Context<TerminalView>) + 'static,
    ) -> Self {
        self.bell_callback = Some(Box::new(callback));
        self
    }

    /// Set a callback to be invoked when the terminal title changes.
    ///
    /// The callback receives a mutable reference to the window and context,
    /// along with the new title string.
    ///
    /// # Arguments
    ///
    /// * `callback` - A function that will be called with the new title
    ///
    /// # Example
    ///
    /// ```ignore
    /// terminal.with_title_callback(|window, cx, title| {
    ///     // Update window title or tab title
    /// })
    /// ```
    pub fn with_title_callback(
        mut self,
        callback: impl Fn(&mut Window, &mut Context<TerminalView>, &str) + 'static,
    ) -> Self {
        self.title_callback = Some(Box::new(callback));
        self
    }

    /// Set a callback to be invoked when the terminal wants to store data to the clipboard.
    ///
    /// The callback receives a mutable reference to the window and context,
    /// along with the text to store. This is typically triggered by OSC 52 escape sequences.
    ///
    /// # Arguments
    ///
    /// * `callback` - A function that will be called with the text to store
    ///
    /// # Example
    ///
    /// ```ignore
    /// terminal.with_clipboard_store_callback(|window, cx, text| {
    ///     // Store text to system clipboard
    /// })
    /// ```
    pub fn with_clipboard_store_callback(
        mut self,
        callback: impl Fn(&mut Window, &mut Context<TerminalView>, &str) + 'static,
    ) -> Self {
        self.clipboard_store_callback = Some(Box::new(callback));
        self
    }

    /// Set a callback to be invoked when the terminal process exits.
    ///
    /// The callback receives a mutable reference to the window and context,
    /// allowing you to close the terminal view or show an exit message.
    ///
    /// # Arguments
    ///
    /// * `callback` - A function that will be called when the process exits
    ///
    /// # Example
    ///
    /// ```ignore
    /// terminal.with_exit_callback(|window, cx| {
    ///     // Close the terminal tab or show exit message
    /// })
    /// ```
    pub fn with_exit_callback(
        mut self,
        callback: impl Fn(&mut Window, &mut Context<TerminalView>) + 'static,
    ) -> Self {
        self.exit_callback = Some(Box::new(callback));
        self
    }

    /// Background thread that reads from stdout.
    ///
    /// This function runs in a background thread, continuously reading bytes
    /// from the stdout reader and sending them through the async channel.
    /// The async channel allows the main async task to be woken up immediately
    /// when data arrives (push-based).
    fn read_stdout_blocking<R: Read + Send + 'static>(
        mut stdout_reader: R,
        bytes_tx: flume::Sender<Vec<u8>>,
    ) {
        let mut buffer = [0u8; 4096];

        loop {
            match stdout_reader.read(&mut buffer) {
                Ok(0) => {
                    // EOF - channel will be dropped, signaling completion
                    break;
                }
                Ok(n) => {
                    // Send bytes to the async task
                    let bytes = buffer[..n].to_vec();
                    if bytes_tx.send(bytes).is_err() {
                        break; // Channel closed
                    }
                }
                Err(_) => {
                    // Read error
                    break;
                }
            }
        }
    }

    /// Handle keyboard input events.
    ///
    /// Converts GPUI keystrokes to terminal escape sequences and writes them
    /// to the stdin writer. If a key handler is set and returns true, the event
    /// is consumed and not sent to the terminal.
    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        // Check if key handler wants to consume this event
        if let Some(ref handler) = self.key_handler
            && handler(event)
        {
            // onehand patch: see `consume` below -- a key this view answers must
            // not also reach the installed input handler.
            cx.stop_propagation();
            return; // Event consumed by handler
        }

        // onehand patch: a key this view turns into bytes is *finished* here.
        //
        // The grid now installs an input handler so that input methods work,
        // and the platform's rule for a key nobody claimed is to hand its
        // character to that handler. Letting the key propagate after writing it
        // to the child therefore types everything twice. Stopping propagation is
        // what draws the line: keys the terminal encodes are the terminal's,
        // keys it does not encode fall through to the input method, which is
        // exactly the split that makes both work.
        let consume = |cx: &mut Context<Self>| cx.stop_propagation();

        // onehand patch: clipboard bindings.
        //
        // `Ctrl+Shift+C/V`, not plain `Ctrl+C/V`: plain Ctrl+C is SIGINT and
        // plain Ctrl+V is the literal-next quote, and a terminal that stole
        // either would be broken in a way no setting could fix.
        let m = &event.keystroke.modifiers;
        if m.control && m.shift {
            match event.keystroke.key.as_str() {
                "c" => {
                    if let Some(text) = self.selection_text() {
                        if !text.is_empty() {
                            if let Ok(mut clipboard) = crate::clipboard::Clipboard::new() {
                                let _ = clipboard.copy(&text);
                            }
                        }
                    }
                    consume(cx);
                    return;
                }
                "v" => {
                    if let Ok(mut clipboard) = crate::clipboard::Clipboard::new() {
                        if let Ok(text) = clipboard.paste() {
                            self.write_paste(&text);
                        }
                    }
                    consume(cx);
                    cx.notify();
                    return;
                }
                _ => {}
            }
        }

        if let Some(bytes) = keystroke_to_bytes(&event.keystroke, self.state.mode()) {
            self.write_typed(&bytes);
            consume(cx);
            cx.notify();
        }
    }

    /// onehand patch: send typed bytes to the child.
    ///
    /// Shared by the keyboard and the input method, because both are the user
    /// typing and both owe the viewport the same courtesy: typing into a
    /// terminal whose viewport is parked up in the scrollback looks like the
    /// keystroke did nothing, and a stale selection left highlighted over text
    /// that has since scrolled says the wrong thing about what would be copied.
    fn write_typed(&mut self, bytes: &[u8]) {
        self.state.with_term_mut(|term| {
            term.scroll_display(Scroll::Bottom);
            term.selection = None;
        });
        let mut writer = self.stdin_writer.lock();
        let _ = writer.write_all(bytes);
        let _ = writer.flush();
    }

    /// onehand patch: write bytes the *terminal* is saying on its own behalf.
    ///
    /// Two kinds go out this way -- an answer to something the child asked, and
    /// a report of what the mouse did -- and neither is typing. That is the
    /// whole difference from [`Self::write_typed`]: nobody touched the keyboard,
    /// so the viewport should stay where the reader parked it and a selection
    /// they made should survive. Snapping to the bottom on a device attributes
    /// query would throw a reader out of the scrollback for something they had
    /// no part in.
    fn write_report(&mut self, bytes: &[u8]) {
        let mut writer = self.stdin_writer.lock();
        let _ = writer.write_all(bytes);
        let _ = writer.flush();
    }

    // ── onehand patch: mouse, scrolling and the clipboard ───────────────────

    /// Where the pointer is, in grid cells.
    ///
    /// Clamped rather than rejected: a drag that runs past the last column
    /// should select to the end of the line, not stop tracking.
    fn cell_at(&self, position: Point<Pixels>) -> (AlacPoint, Side) {
        let (row, col, side) = self.grid_position(position);
        let display_offset = self.state.with_term(|term| term.grid().display_offset()) as i32;
        (
            AlacPoint::new(Line(row as i32 - display_offset), Column(col)),
            side,
        )
    }

    /// Where the pointer is, as a cell of the *viewport*.
    ///
    /// The other half of [`Self::cell_at`], and the difference is who is being
    /// told. A selection is a range of the buffer, so it has to survive the
    /// viewport moving underneath it; a mouse report is a coordinate on the
    /// screen the child believes it is drawing, and the child knows nothing
    /// about a scrollback it does not own.
    fn viewport_cell(&self, position: Point<Pixels>) -> AlacPoint {
        let (row, col, _) = self.grid_position(position);
        AlacPoint::new(Line(row as i32), Column(col))
    }

    /// The row, column and half-cell the pointer is over.
    ///
    /// Clamped rather than rejected: a drag that runs past the last column
    /// should select to the end of the line, not stop tracking.
    fn grid_position(&self, position: Point<Pixels>) -> (usize, usize, Side) {
        let (cw, ch): (f32, f32) = (
            self.renderer.cell_width.into(),
            self.renderer.cell_height.into(),
        );
        let x: f32 = (position.x - self.content_origin.x).into();
        let y: f32 = (position.y - self.content_origin.y).into();

        let (cols, rows) = (self.state.cols(), self.state.rows());
        let col = ((x / cw.max(1.0)).floor().max(0.0) as usize).min(cols.saturating_sub(1));
        let row = ((y / ch.max(1.0)).floor().max(0.0) as usize).min(rows.saturating_sub(1));

        // Which half of the cell decides whether the boundary belongs to this
        // cell or the next -- the difference between selecting a character and
        // selecting up to it.
        let side = if (x / cw.max(1.0)).fract() < 0.5 {
            Side::Left
        } else {
            Side::Right
        };

        (row, col, side)
    }

    /// onehand patch: the modifier bits a report carries.
    ///
    /// Shift is never among them, and that is not an omission. Shift is the key
    /// that takes a gesture *back* from the child -- see
    /// [`Self::child_tracks_mouse`] -- so a report can only be on its way at all
    /// when shift was not held, and a bit saying otherwise would be a lie the
    /// receiving program acts on.
    fn report_modifiers(modifiers: &Modifiers) -> u8 {
        encode_modifiers(false, modifiers.alt, modifiers.control)
    }

    /// onehand patch: whether this gesture belongs to the child rather than to
    /// the terminal.
    ///
    /// **Shift is the way back.** A program that turns on mouse tracking takes
    /// the pointer completely: click, drag and wheel all become its input, which
    /// leaves no way to select a line of its output and copy it -- and needing
    /// to do that is most of why someone is looking at the output. Holding shift
    /// is the convention every terminal shares for saying "this one is mine",
    /// and it costs the child nothing, because a program cannot see shift+click
    /// in any terminal and so has never been written to want it.
    fn child_tracks_mouse(&self, modifiers: &Modifiers) -> bool {
        !modifiers.shift && self.state.mode().intersects(TermMode::MOUSE_MODE)
    }

    /// Hand a press to the child, or start a selection.
    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // onehand patch: `Window::focus` now takes the app context too.
        window.focus(&self.focus_handle, cx);

        // onehand patch: mouse reporting.
        if self.child_tracks_mouse(&event.modifiers) {
            let point = self.viewport_cell(event.position);
            let bytes = button_report(
                event.button,
                true,
                point,
                Self::report_modifiers(&event.modifiers),
                self.state.mode(),
            );
            if let Some(bytes) = bytes {
                self.write_report(&bytes);
                self.reported_button = Some(event.button);
                self.reported_cell = Some(point);
                // A selection left highlighted under a screen the child is
                // about to redraw describes text that is no longer there.
                self.state.with_term_mut(|term| term.selection = None);
                cx.notify();
                return;
            }
        }

        // Selection is the left button's alone: the middle and right buttons
        // have no meaning here once the child has declined the press, and a
        // right-click that wiped the selection would take away the thing the
        // user was about to copy.
        if event.button != MouseButton::Left {
            return;
        }

        let (point, side) = self.cell_at(event.position);
        // Click count picks the granularity, the convention every terminal
        // shares: 1 = characters, 2 = words, 3 = lines.
        let ty = match event.click_count {
            2 => SelectionType::Semantic,
            3 => SelectionType::Lines,
            _ => SelectionType::Simple,
        };
        self.dragging = true;
        self.state.with_term_mut(|term| {
            term.selection = Some(Selection::new(ty, point, side));
        });
        cx.notify();
    }

    /// Hand a release to the child, or finish a drag and copy what it selected.
    fn on_mouse_up(&mut self, event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        // onehand patch: the release is reported for whichever button's *press*
        // was handed over, and not for the one the event names. They are the
        // same button in every ordinary case, and where they are not -- a second
        // button pressed and let go during a drag -- reporting the press without
        // its release leaves the child believing a button is still down.
        if let Some(button) = self.reported_button.take() {
            let bytes = button_report(
                button,
                false,
                self.viewport_cell(event.position),
                Self::report_modifiers(&event.modifiers),
                self.state.mode(),
            );
            if let Some(bytes) = bytes {
                self.write_report(&bytes);
            }
            self.reported_cell = None;
            cx.notify();
            return;
        }

        if !self.dragging {
            return;
        }
        self.dragging = false;

        // Copy-on-select, the Linux convention. PRIMARY is the right target,
        // but `arboard` does not expose it portably, so this writes the regular
        // clipboard only when the selection is non-empty -- an empty drag must
        // never clobber what the user already had there.
        if let Some(text) = self.selection_text() {
            if !text.is_empty() {
                if let Ok(mut clipboard) = crate::clipboard::Clipboard::new() {
                    let _ = clipboard.copy(&text);
                }
            }
        }
        cx.notify();
    }

    /// Report motion to the child, or extend a live selection.
    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // onehand patch: two different gestures reach here. A drag whose press
        // we handed over stays the child's for as long as the button is down --
        // that is what a drag *is* to a program tracking the mouse. And under
        // mode 1003 a pointer with no button down at all is still news, which is
        // how a menu highlights the entry it is over.
        let childs_gesture = self.reported_button.is_some()
            || (!self.dragging
                && !event.modifiers.shift
                && self.state.mode().contains(TermMode::MOUSE_MOTION));

        if childs_gesture {
            let point = self.viewport_cell(event.position);
            // Checked before the report is built, not after: this is the path a
            // moving pointer takes on every frame.
            if self.reported_cell != Some(point) {
                let bytes = motion_report(
                    self.reported_button,
                    point,
                    Self::report_modifiers(&event.modifiers),
                    self.state.mode(),
                );
                if let Some(bytes) = bytes {
                    self.reported_cell = Some(point);
                    self.write_report(&bytes);
                }
            }
            return;
        }

        if !self.dragging {
            return;
        }
        let (point, side) = self.cell_at(event.position);
        self.state.with_term_mut(|term| {
            if let Some(selection) = term.selection.as_mut() {
                selection.update(point, side);
            }
        });
        cx.notify();
    }

    /// Scroll the scrollback.
    fn on_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ch: f32 = self.renderer.cell_height.into();
        // Touchpads report pixels and wheels report lines; both have to end up
        // as the same number of grid rows or the two feel nothing alike.
        let lines = match event.delta {
            ScrollDelta::Lines(delta) => delta.y as i32,
            ScrollDelta::Pixels(delta) => {
                let dy: f32 = delta.y.into();
                (dy / ch.max(1.0)).round() as i32
            }
        };
        if lines == 0 {
            return;
        }

        // onehand patch: the wheel is the child's where it is tracking the
        // mouse, and *also* where it is on the alternate screen and is not --
        // there the terminal has no scrollback to move, so a wheel it kept for
        // itself would do nothing at all. Which of the two, and what the bytes
        // are, is decided in one place.
        if !event.modifiers.shift {
            let bytes = scroll_report(
                lines,
                self.viewport_cell(event.position),
                Self::report_modifiers(&event.modifiers),
                self.state.mode(),
            );
            if let Some(bytes) = bytes {
                self.write_report(&bytes);
                cx.notify();
                return;
            }
        }

        self.state
            .with_term_mut(|term| term.scroll_display(Scroll::Delta(lines)));
        cx.notify();
    }

    /// The selected text, if any.
    fn selection_text(&self) -> Option<String> {
        self.state.with_term(|term| term.selection_to_string())
    }

    /// Write `text` to the PTY the way a paste has to be written.
    ///
    /// Two things upstream did not do, both of which matter: bracketed paste
    /// (without the markers an editor autoindents every pasted line into a
    /// staircase) and newline normalisation (a terminal expects `\r`, and a
    /// clipboard full of `\n` submits one line at a time in a shell).
    fn write_paste(&mut self, text: &str) {
        let bracketed = self
            .state
            .mode()
            .contains(alacritty_terminal::term::TermMode::BRACKETED_PASTE);

        let mut out: Vec<u8> = Vec::with_capacity(text.len() + 12);
        if bracketed {
            out.extend_from_slice(b"\x1b[200~");
            // A pasted end-marker would let the payload break out of the paste
            // and be read as keystrokes.
            out.extend_from_slice(text.replace("\x1b[201~", "").as_bytes());
            out.extend_from_slice(b"\x1b[201~");
        } else {
            out.extend_from_slice(text.replace("\r\n", "\r").replace('\n', "\r").as_bytes());
        }

        let mut writer = self.stdin_writer.lock();
        let _ = writer.write_all(&out);
        let _ = writer.flush();
    }

    /// Process pending terminal events.
    ///
    /// This method drains all available events from the event receiver
    /// and handles them appropriately. Note: bytes are processed in the
    /// async reader task, not here.
    fn process_events(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Process terminal events (from alacritty event proxy)
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                TerminalEvent::Wakeup => {
                    // Terminal has new content - already handled by async task
                }
                TerminalEvent::Bell => {
                    if let Some(ref callback) = self.bell_callback {
                        callback(window, cx);
                    }
                }
                TerminalEvent::Title(title) => {
                    if let Some(ref callback) = self.title_callback {
                        callback(window, cx, &title);
                    }
                }
                TerminalEvent::ClipboardStore(text) => {
                    if let Some(ref callback) = self.clipboard_store_callback {
                        callback(window, cx, &text);
                    }
                }
                // onehand patch: reading the clipboard is deliberately not
                // answered.
                //
                // The request is `OSC 52` with a `?` payload, and honouring it
                // hands whatever the user last copied -- a password, a token, a
                // paragraph from another window -- to whatever is running in
                // this terminal, including something on the far end of an ssh
                // session. That is why xterm ships it disabled and alacritty
                // dropped it outright. The write half is answered
                // (`ClipboardStore` above): a program can put text on the
                // clipboard because a person asked it to, and cannot take text
                // off one.
                TerminalEvent::ClipboardLoad => {}
                // onehand patch: an answer the child is blocked on. Written
                // straight out rather than through `write_typed`, which snaps
                // the viewport to the bottom and drops the selection -- correct
                // for a keystroke, and wrong for a reply the user did not make:
                // a program probing the terminal would yank the reader out of
                // the scrollback they were sitting in.
                TerminalEvent::PtyReply(text) => {
                    self.write_report(text.as_bytes());
                }
                // onehand patch: resolved against the palette in force right
                // now, which is why this is answered here and not in the proxy.
                TerminalEvent::ColorQuery { index, reply } => {
                    let rgb = self
                        .state
                        .with_term(|term| self.renderer.palette.rgb_at(index, term.colors()));
                    self.write_report(reply(rgb).as_bytes());
                }
                TerminalEvent::Exit => {
                    if let Some(ref callback) = self.exit_callback {
                        callback(window, cx);
                    }
                }
            }
        }
    }

    /// Get the current terminal dimensions.
    ///
    /// # Returns
    ///
    /// A tuple of (columns, rows).
    pub fn dimensions(&self) -> (usize, usize) {
        (self.state.cols(), self.state.rows())
    }

    /// Resize the terminal to new dimensions.
    ///
    /// This method should be called when the terminal view size changes.
    /// It updates the internal grid and notifies the terminal process of the new size.
    ///
    /// # Arguments
    ///
    /// * `cols` - New number of columns
    /// * `rows` - New number of rows
    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.state.resize(cols, rows);
    }

    /// Get the current terminal configuration.
    ///
    /// # Returns
    ///
    /// A reference to the current configuration.
    pub fn config(&self) -> &TerminalConfig {
        &self.config
    }

    /// Get the focus handle for this terminal view.
    ///
    /// # Returns
    ///
    /// A reference to the focus handle.
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    /// Update the terminal configuration.
    ///
    /// This method updates the terminal's configuration, including font settings,
    /// padding, and color palette. Changes take effect on the next render.
    ///
    /// # Arguments
    ///
    /// * `config` - The new configuration to apply
    /// * `cx` - The context for triggering a repaint
    pub fn update_config(&mut self, config: TerminalConfig, cx: &mut Context<Self>) {
        // Update renderer with new font settings and palette
        self.renderer.font_family = config.font_family.clone();
        self.renderer.font_size = config.font_size;
        self.renderer.line_height_multiplier = config.line_height_multiplier;
        self.renderer.palette = config.colors.clone();

        // Store the new config
        self.config = config;

        // Trigger a repaint - cell dimensions will be recalculated via measure_cell()
        cx.notify();
    }

    /// Calculate terminal dimensions from pixel bounds and cell size.
    ///
    /// Helper method to determine how many columns and rows fit in the given bounds.
    #[allow(dead_code)]
    fn calculate_dimensions(&self, bounds: Bounds<Pixels>) -> (usize, usize) {
        let width_f32: f32 = bounds.size.width.into();
        let height_f32: f32 = bounds.size.height.into();
        let cell_width_f32: f32 = self.renderer.cell_width.into();
        let cell_height_f32: f32 = self.renderer.cell_height.into();

        let cols = ((width_f32 / cell_width_f32) as usize).max(1);
        let rows = ((height_f32 / cell_height_f32) as usize).max(1);
        (cols, rows)
    }
}

/// onehand patch: the grid as a text field, so input methods work.
///
/// Without this the platform never opens an input context over the terminal,
/// and an input method that composes -- Vietnamese telex, pinyin, kana --
/// stays in passthrough: the raw keys reach the child, so typing "khoong"
/// puts *khoong* on the command line instead of "không". The trait is also
/// what places the candidate list, through [`Self::bounds_for_range`].
///
/// The methods that describe a document all answer for the composition alone,
/// which is the only text this view owns. Everything committed has already
/// gone to the child and belongs to it, not to us: a terminal cannot hand back
/// a range of its screen as editable text, and claiming otherwise would invite
/// the input method to try to replace it.
impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        range: std::ops::Range<usize>,
        adjusted: &mut Option<std::ops::Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let len = self.preedit.chars().count();
        let start = range.start.min(len);
        let end = range.end.min(len);
        if start != range.start || end != range.end {
            *adjusted = Some(start..end);
        }
        Some(
            self.preedit
                .chars()
                .skip(start)
                .take(end.saturating_sub(start))
                .collect(),
        )
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        // The caret always sits at the end of the composition: there is no
        // caret movement inside a terminal's pending syllable.
        let end = self.preedit.encode_utf16().count();
        Some(UTF16Selection {
            range: end..end,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<std::ops::Range<usize>> {
        if self.preedit.is_empty() {
            return None;
        }
        Some(0..self.preedit.encode_utf16().count())
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.preedit.clear();
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        _range: Option<std::ops::Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The composition is finished, so it stops being ours and becomes the
        // child's. Cleared first: a commit that left the preedit drawn would
        // paint the same syllable twice, once over the cursor and once in the
        // cell the child echoed it into.
        self.preedit.clear();
        // A terminal's Return is carriage return; a line feed here would ask
        // for a new line without asking to run the command.
        let bytes = text.replace("\r\n", "\r").replace('\n', "\r");
        if !bytes.is_empty() {
            self.write_typed(bytes.as_bytes());
        }
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range: Option<std::ops::Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<std::ops::Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        new_text.clone_into(&mut self.preedit);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: std::ops::Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // The element's own bounds would put the candidate list at the top-left
        // of the whole panel; what the user is reading is the cursor.
        Some(self.preedit_bounds)
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        // Answering would mean claiming a character index for a click anywhere
        // on the screen, and the screen is the child's output, not a document.
        None
    }
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Process any pending events
        self.process_events(window, cx);

        // Get terminal state and renderer for rendering
        let state_arc = self.state.term_arc();
        let renderer = self.renderer.clone();
        let resize_callback = self.resize_callback.clone();
        let padding = self.config.padding;
        // onehand patch: mouse events are in window space and the grid is in
        // cell space; only the paint knows where the content rect landed.
        let view = cx.entity().downgrade();
        // onehand patch: the input handler is installed during paint and needs
        // the entity by strong reference; the composition is read here so the
        // paint closure does not have to reach back into the view.
        let entity = cx.entity();
        let focus_handle = self.focus_handle.clone();
        let preedit = self.preedit.clone();
        // onehand patch: read here rather than inside the paint closure, which
        // is handed the app context and not the window that owns focus. The
        // cursor is drawn hollow without it.
        let focused = self.focus_handle.is_focused(window);

        div()
            .size_full()
            .bg(rgb(0x1e1e1e))
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            // onehand patch: all three buttons, not just the left one. Only the
            // left starts a selection, but a program tracking the mouse is
            // entitled to hear about every button -- the right one opens a
            // context menu in a file manager and the middle one closes a tab in
            // a tabbed pager, and a terminal that swallows them makes those
            // features look broken rather than absent.
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .child(
                canvas(
                    move |bounds, _window, cx| {
                        let origin = Point {
                            x: bounds.origin.x + padding.left,
                            y: bounds.origin.y + padding.top,
                        };
                        let _ = view.update(cx, |view: &mut Self, _| {
                            view.content_origin = origin;
                        });
                        bounds
                    },
                    move |bounds, _, window, cx| {
                        use alacritty_terminal::grid::Dimensions;

                        // Measure actual cell dimensions from the font
                        let mut measured_renderer = renderer.clone();
                        measured_renderer.measure_cell(window);

                        // Calculate available space after padding
                        let available_width: f32 =
                            (bounds.size.width - padding.left - padding.right).into();
                        let available_height: f32 =
                            (bounds.size.height - padding.top - padding.bottom).into();
                        let cell_width_f32: f32 = measured_renderer.cell_width.into();
                        let cell_height_f32: f32 = measured_renderer.cell_height.into();

                        let cols = ((available_width / cell_width_f32) as usize).max(1);
                        let rows = ((available_height / cell_height_f32) as usize).max(1);

                        // Helper struct implementing Dimensions for resize
                        struct TermSize {
                            cols: usize,
                            rows: usize,
                        }
                        impl Dimensions for TermSize {
                            fn total_lines(&self) -> usize {
                                self.rows
                            }
                            fn screen_lines(&self) -> usize {
                                self.rows
                            }
                            fn columns(&self) -> usize {
                                self.cols
                            }
                            fn last_column(&self) -> alacritty_terminal::index::Column {
                                alacritty_terminal::index::Column(self.cols.saturating_sub(1))
                            }
                            fn bottommost_line(&self) -> alacritty_terminal::index::Line {
                                alacritty_terminal::index::Line(self.rows as i32 - 1)
                            }
                            fn topmost_line(&self) -> alacritty_terminal::index::Line {
                                alacritty_terminal::index::Line(0)
                            }
                        }

                        // Resize terminal if dimensions changed
                        let mut term = state_arc.lock();
                        let current_cols = term.columns();
                        let current_rows = term.screen_lines();
                        if cols != current_cols || rows != current_rows {
                            // Notify the PTY about the resize
                            if let Some(ref callback) = resize_callback {
                                callback(cols, rows);
                            }
                            term.resize(TermSize { cols, rows });
                        }

                        // Paint the terminal with measured dimensions
                        measured_renderer.paint(bounds, padding, &term, focused, window, cx);

                        // onehand patch: the input method, drawn last so the
                        // composition sits over the grid, and registered here
                        // because an input handler may only be installed during
                        // paint. Registered on every paint whether or not
                        // anything is being composed: the handler is what tells
                        // the platform this widget takes text at all, so an
                        // input method has somewhere to attach *before* the
                        // first key of a syllable is pressed.
                        let area = measured_renderer
                            .paint_preedit(bounds, padding, &term, &preedit, window, cx);
                        drop(term);
                        let _ = entity.update(cx, |view: &mut Self, _| {
                            view.preedit_bounds = area;
                        });
                        window.handle_input(
                            &focus_handle,
                            ElementInputHandler::new(bounds, entity),
                            cx,
                        );
                    },
                )
                .size_full(),
            )
    }
}

// Tests are omitted due to macro expansion issues with the test attribute
// in this configuration. Integration tests can be added separately.
