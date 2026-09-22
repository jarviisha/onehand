# DESIGN — Answer / Transcript UI

The design language of the **agent pane transcript** (the "answer" area): every
block type the chat renders, how it folds, and how it behaves mid-stream.

> **Structure lives here; values do not.** This file used to carry the
> transcript's palette, its px sizes and its radius ladder, mirrored by hand into
> `theme.rs`. Decision **D1** (DECISIONS.md) ended that — gpui-component's theme
> plus one surface-ramp override is the look. So what follows describes
> **anatomy and behaviour**: which
> blocks exist, what each is made of, what folds, what is bounded. Every colour,
> radius and size is read from `cx.theme()` at the call site
> ([DESIGN.md](DESIGN.md) §3–§4), and the numeric caps live as named constants
> next to the renderers they bound (§8).
>
> The renderer is [crates/app/src/chat/transcript.rs](crates/app/src/chat/transcript.rs);
> the model it draws is `onehand_core::chat` (P3-A moved it there, so both the
> model and this document outlived the front end they were written for).

**Transcript model:**

1. **Two sides, and the side is the label.** The user's prompt is a filled
   bubble against the right edge, shrunk to what was typed; everything the agent
   produces starts at the left. Told apart by which edge they hang off, so
   finding the last question in a long conversation takes no reading. Nothing
   else in the transcript is ever right-aligned — a second right-hand block
   would make the side mean "somebody's block" instead of "the user's".
2. **The user's question stands out.** The user message is the *one* filled
   block. Agent prose runs bare so code and diffs get the full width.
3. **One turn = many blocks.** An agent turn is prose + tool cards + process rows
   stacked on one shared left axis.

The transcript runs in a centred **59rem reading column**, held off the panel by
a single margin. That is a maximum rather than a fixed minimum: on a narrower
panel the column contracts to the available width, and below about 30rem the
margin comes down a step because past that point it is taking room from the line
rather than framing it. **It is derived from the widest thing it has to
hold, not chosen for prose.** A column set by reading measure alone is narrower —
and what that costs is everything in the transcript that is *not* prose, which is
what somebody is reading it for when something has gone wrong. A diff is the
sharpest case: at the prose-sized cap this replaced, a line of this project's own
code had 74 columns to sit in against the 100 `rustfmt` writes it at, so nearly
every line of nearly every diff wrapped. So the number is the width at which 100
mono columns clear the chrome around them — the child inset, the detail's
gutters, the well's padding, the diff's sign column, the frame's border — and a
test holds the sum, so the cap moves when any of those insets does instead of
the diff quietly getting tighter. The blocks that still want more have answers of
their own: a command scrolls sideways inside its well, a file name is elided from
the front where the identifying tail survives, and a row's least load-bearing
column gives way first.
**The composer is capped narrower**, and its popups with it, because a message
being written is not a
message being read: the reading column is set by how far a line of prose can run
before the eye loses its place coming back, while the composer holds a few lines
at most and has its controls at the two ends of one row — at the full column
those ended a hand's width apart with nothing between them. Narrower, the row
reads as one control strip and the card reads as something resting on the
conversation rather than as its last paragraph. **Everything pinned above the
composer takes the composer's cap too** — a parked permission, a parked
question, a queued prompt — because while they are pinned they are part of that
stack: they rest directly on the card, share its surface and its radius, and are
read as one object with it. A card an inch wider than the box it sits on reads
as two panels that failed to line up. Both caps still leave the surfaces the
same inset rhythm inside themselves. Content wider than its own well scrolls
there. Mono is only for what a machine produced. Separate with hairlines, not
fills; if a colour is not carrying meaning, it is muted.

Items marked *(not rendered)* are contract items this build does not draw. They
are listed because leaving them out silently is how a contract quietly becomes
fiction.

---

## 1 — Frame & rhythm

The transcript is a **full-height** scrolling column inside the dock's centre
panel, running from the header's hairline to the bottom of the pane, with the
composer floating over its foot.

**One scale, named for jobs, and nothing sized off it.** Every gap, pad, inset,
height and corner in the transcript comes from a single ladder held in one
place, and each step is named for what it is for rather than for how big it is:
the space between turns, between blocks of one turn, between the parts of one
block, between a line and its caption, and the pair of insets a frame keeps.
Written out per call site they had already come apart — two paragraphs of one
answer standing further apart than the answer stood from the card under it, a
step's detail inset to one number while the group holding it used another, four
values in use for the one job of "a line and the caption under it".

The ladder's rule is that **what is inside a thing is always closer than what
surrounds it**, at every level and for corners as much as for gaps. That is the
whole of how the transcript says where one block ends and the next begins, and
it is why a value chosen between two steps is a boundary nobody can resolve. The
corner ladder is anchored on the theme's own two named radii, so a theme that
squares its corners squares every one of these with it — and it is deliberately
**small and nearly square all the way up**: a mark, a control, a block, and the
user's bubble. What the transcript is meant to read as is a technical document,
and a generous corner is what turns a record into a feed of cards. That includes
the status pill, which is a corner here rather than a capsule: fully round it
was the only curved silhouette in the column, reading as a badge stuck onto a
row rather than as one of its columns. Two tests hold the scale: one fails on
any step that has left it, naming the step; the other on any pair that has
stopped nesting.

**Nothing is nested more than two frames deep.** A block's rows are inside the
block's own edge and bring none of their own; a well inside a row's detail is
the second and last. Where more separation is wanted inside a frame, it is a
hairline or white space — never a third box.

The **composer is a card** — a hairline and a radius around the field and every
control that acts on it — inset from the panel's edges, not a region divided off
by a rule across the pane. The two say different things: a rule says the pane
ends here, a card says this is the message being written, which is what its
contents are about.

**Everything that floats over the composer is one surface**, built in one
place: the composer card, a parked permission, a parked question, an adapter
still connecting and a prompt waiting its turn. They arrive in one column,
stacked, and read as a single object — so they share the raised opaque fill, the
hairline, the theme's named card radius and the shadow. Written out per card
they came apart exactly where copies do: two sat on the reading surface with a
single radius while the others floated with a shadow and a doubled one, and a
permission parked above a queued prompt read as two unrelated things rather than
as the same kind of interruption twice. The transcript's own side inset is read
from that same radius, so the two surfaces keep one spacing rhythm.

A blocking card **keeps the raised surface when it is drawn back in the
transcript**, answered: the same element is used in both places by design, and
what it carries into the history is the mark of the one block that stopped
everything until somebody replied.

**Its width follows where it is, not what it is.** Pinned, it is the composer's
column; answered and drawn in the transcript, it is the transcript's, like every
block around it. That was already half true — a transcript row is inset inside
the reading column while a pinned card never was — so a rule saying the two
widths must match was describing something the layout had never quite done. What
does have to hold is the **text size**, which is the transcript's in both
places: a question re-read in the history has to be the same words at the same
weight as the question that stopped everything.

It **floats as a real overlay.** A row of its own would take height out of the
conversation, and that height changes on almost every keystroke — the field
grows, the attachment tray appears, a parked permission pins another card above
it — so the transcript would shift while it was being read. The full-width
overlay wrapper is transparent while its surfaces share the transcript's
centred reading column. Only the composer, its popups, and its pinned cards are
opaque, so text directly behind an interactive surface never competes with it.

The transcript is **not clipped short** to make room. It runs the full height
and *ends* above the composer, by padding inside the scroll equal to the
composer’s measured height plus the rest the last row comes to — so the last
line stops clear of the box, and nothing is unreachable behind it. That rest is
**a turn's worth of air, not a hairline**: the composer is a surface of its own,
and a conversation that stops just short of it reads as one still trying to fit,
the last line of the answer and the box it is answered in running together into
one block.

**Where it stops being drawn is the composer's own middle.** The overlay is
transparent around its surfaces, so a row scrolled under it stayed visible in
the strip above the card, at both sides of it and under it — a line of the
conversation cut in two by a box resting on top of it, which reads as the card
having been dropped on the text rather than as the text ending. Clipped at the
middle it ends behind the card's opaque top half, so nothing is ever seen
sliced: the cut itself is under a surface. The scroll is not shortened by this —
the padding above still ends the conversation clear of the box, and nothing is
unreachable behind it.

**And it dissolves into that clip rather than stopping on it.** A clip on its
own is a line: text at full strength for one row and gone the next. Behind the
card that reads as occlusion, which is what it is; either side of the card, and
in the gap above it, it reads as a rendering fault. So the conversation fades
into the surface under it over the last few lines of prose before the cut — long
enough that the eye never finds an edge, where a shorter run only blurs one.
The fade is drawn **between the conversation and every control**, so what it
takes is the transcript alone: the jump pill, the pinned cards and the composer
all come after it, each carrying its own opaque surface. And it ends **at the
clip**, not at the top of the composer, because the card is narrower than the
panel — a fade stopping at the card's own edge would leave the strips either
side of it showing full-strength text for the height of the card.

**A question just asked goes to the top of the panel and stays there while it
is answered.** The transcript scrolls so the new prompt rests on the same head
padding the first row of the conversation would, and the answer arrives in the
space beneath it — the question stays legible for as long as it fits on screen
with its answer, instead of being pushed off the top by the first paragraph.

Holding one is only possible because the transcript keeps **room under the
turn** while it does. A list aligned to its bottom pulls its content back down
as soon as that content stops filling the view, so a question with nothing under
it yet can only sit at the top of the panel if something scrollable is standing
under it. That room is the panel minus the turn, so **it shrinks by exactly what
the turn grows**: the answer drifts down into the space beneath the question
instead of the whole column sliding, and nothing on screen moves that the answer
did not write.

Which is what makes the end of the hold invisible. The room bottoms out at the
transcript's ordinary floor exactly when the turn reaches the composer, and at
that one height *the question at the top* and *the last line above the composer*
are the same picture — so the list is handed back to **following its tail**
there, and from then on the transcript scrolls with the stream. There is no
jump, because there is nothing left to jump between.

**Scrolling ends the hold as a scroll, not as a layout.** A wheel, a drag or a
jump to a find hit takes the position over — the hold was for the question
arriving, not a place the reader has to fight — but the room under the turn
stays exactly the size it was. Taking it away underneath somebody who has just
scrolled would move the conversation while they are reading it, which is the one
thing they scrolled to stop. Coming back to the question takes the position back
again: this is not a latch, and the room outlasts both.

**The held position is also the end of the transcript.** The room is the panel
minus the turn, so the conversation runs out exactly where the question meets
the top edge and there is nothing under it to scroll into. A short answer
therefore has two names for one place — *resting on the question* and *at the
bottom* — and anything reading the scroll position has to accept both, or a
single notch of the wheel reads as the reader having walked off the end and the
room is taken away under a turn that still needs it.

While a question is held and the view is resting on it, **the jump-to-the-latest
control stays hidden** — the reader has not scrolled anywhere, and the activity
it would return them to is arriving in the space they are already looking at. It
comes back the moment they scroll off the question, and it takes them to the
question rather than to the tail, because with a room under the turn those are
the same place.

**A bare strip sits under the composer card**, outside it and with no chrome of
its own: the project's branch and change count on the left, the permission mode
on the right. The card is the message being written and everything inside it
acts on that message; neither of these does — the branch is about the project
and holds across every session in it, and what the agent is allowed to do
without asking outlives the prompt in the field. Resting under the card they
read as the standing state; inside the row they read as part of what is being
typed.

**Left is the project, right is the turn.** The branch is about the repository
and holds across every session in it; the permission mode is what the next turn
runs under. Which side a thing is on is the whole of what says which kind it is.

**The branch is a control, not a label.** Everything a reader might do about
what it says — split it into a second checkout, rename it, look again — is
something the app already does, and printed flat it was the one piece of state
on screen with no way to act on it, with both of those actions reachable only
from a rail row or a page that is not up while a conversation is. It opens the
same kind of menu the rail's rows do. It is **drawn to match the chip opposite**
— same height, same inset, same muted ink, a mark then a word and no caret — so
the strip stays one row of one kind of thing and the only difference between its
two ends is which end they are on.

**Every chip letters its value at full strength, and its mark at the quiet
one** — the branch, the permission mode, Fast mode and Model alike, on the strip
and in the card. What a chip is for is the value in it; the icon, the caret and
the chrome are what the muted ink is for. A chip standing beside another with
its word a shade fainter reads as one that is somehow less settled, rather than
as one about a different question — and which question a chip is about is
already said by which row it is on.

**The two setting chips lead with the mark of the setting and draw no
caret.** The mark is what makes a value legible without reading it, and it is
the *setting* rather than the value: a mark per value would be the app deciding
what an agent-chosen word means, and the word the agent picked is right beside
it anyway. The caret goes because between the chips and the branch's own mark it
would be a third small glyph in a row an inch long, and it is the least of them
— a chip is the only thing near it with a hover fill and a pointer, so what can
be pressed is already said twice. The branch line is core's own sentence, the same one the
rail prints a few inches away — two spellings of one fact is a difference a
reader assumes means something. The strip is **inside the measured overlay**, so
the transcript ends above it rather than behind it, and it is **absent
entirely** where there is neither a repository nor a setting to offer: a rule of
blank space under the composer would be chrome reporting that it has nothing to
report.

The composer presents agent configuration as exactly two option actions:
**Model** and **Mode**. Each names its current value and nothing else — the
setting's own name is in the tooltip, because the value is the only part that
ever changes and the row runs out of width before anything else in the card
does. Mode opens its choices directly. Model opens one flat list
that also includes Effort and every remaining agent-advertised config group;
every visible row is a choice, with no intermediate settings screen. Their
dropdown carets remain the immediate signal that the values can be changed.

**They sit on different rows, because they answer different questions.** Model
is in the card, at the start, with what goes *into* the prompt — the way a file
joins it, the slash commands, and what will answer them. Mode is on the strip
below, with Fast mode, because how the agent is allowed to act is standing state
rather than part of this message. Both open the same card above the composer.

**Send refuses out loud.** Whether a prompt may be sent is the conversation’s
answer, not the view’s (`Chat::submit_blocker`), and the controls carry it.
While a turn is in flight the row carries **Stop** — the arrow replaced by a
pause in the danger tint, and no word, the tint carrying what the word did — and
**Queue** joins it as the primary action the moment there is a draft. Stop is
there at every moment the work is running: it is the one control in this app
that throws running work away, and one that vanished as soon as somebody started
typing would be gone at exactly the moment they had most to say about the turn.
**They are never one button wearing two faces**, which is the rule that holds:
the arrow starts something and the pause ends something, they carry different
tints, and they are never on screen together — Send is drawn only when no turn
is running, and Stop only while one is. What is *not* claimed is that they stand
in different places. The trailing end of the row is the turn's own control
whichever of them is there, and Stop takes the slot the arrow was in a moment
earlier. The cost of that is stated rather than designed away: somebody who
sends and then presses the same spot again stops the turn they just started.
Three things stand between them — the tint, the glyph, and Queue arriving
*labelled* beside Stop the moment there is a draft, so the pair mid-turn is
never two wordless buttons a thumb-width apart. The alternative, a reserved
empty slot so that neither control ever moves, spends a fixed inch of a row that
already runs out of width before anything else in the card does, and spends it
on the state that is idle most of the time. Otherwise
Send is disabled with the reason on it — an
empty buffer, a staged file that could not be read (named), an agent not
connected. Actionable blockers are also written inline under the controls, so
their explanation does not depend on discovering a disabled button's tooltip.
A Send that stays enabled over a prompt the model will discard is a
control that does nothing when pressed and says nothing about why, which reads
as a fault rather than as a rule.

**A popup says when it has nothing.** A trigger that matches no file and no
command still opens the list, carrying one muted *No matches* row: the popup is
the only thing on screen that ever confirms the `@` or `/` was understood, so
vanishing reads as completion being broken. A selector with no choices has
nothing to confirm and stays away. Either list is dismissed by `Esc` or by a
click anywhere outside the composer’s own surfaces. The list is capped, and a
capped list says what it is holding back: cut with nothing admitting it, a query
that matched four hundred files reads as one that matched fifty, and the file
the user is looking for is missing for no visible reason.

**What the popup says *about* the list is held outside the part that scrolls.**
The count of what is being held back and the line naming the keys are sentences
about the list rather than rows in it, and both appear only once the list is
long — so kept among the rows they were scrolled out of sight in precisely the
case that produced them: the count sat past the fiftieth row, and the keys went
away at the moment the list became long enough to need walking. Held against the
surface's own edge, nothing the list does to its offset can move them. **The
bound goes with them, onto the popup rather than onto the list**, or the part
left outside is the part that grows past the top of the panel. *No matches* is
the exception and stays among the rows, because it stands *in place of* them:
there is nothing for it to be scrolled behind. The line naming the keys names
**both** that take the highlighted row — `Tab` beside `Enter` — since a line
listing the keys is read as the complete set.

**Every popup is as wide as the card it opens over**, which is the composer's
column and not the transcript's. A file candidate is a path and wants every inch
of it; a choice wants it too, now that a row carries the agent's own sentence
about what the choice is for. Sized to its own rows instead — which the selector
lists once were, anchored to the chip that opened them — that sentence had
nowhere to go and the rows that did fit came out narrower than the words in
them.

**The list is walked, not just pointed at.** `Up`/`Down` move the highlight and
wrap at both ends, the list **scrolls to keep the highlight on screen**, `Enter`
takes it, and a click takes the row it landed on — a click is a choice already
made, and asking for a second keystroke to confirm it is asking twice. The arrows
belong to the list only while a list is open; the rest of the time they move the
caret in the prompt. A selector opens **on its current value**, not at the top:
the list is a setting's state, and arrowing away from where you are is the
movement the user means.

**One fill in that list, and it means one thing: the row about to be taken.** It
follows the pointer and the arrow keys alike, whichever moved last — hovering a
row takes the highlight off wherever the keyboard left it — so what is lit is
always what `Enter` or a click would pick. The value already in force is said by
a **tick at the row's end** and by nothing else.

The two were drawn apart once: a strong fill for the value in force, a faint one
for the cursor. That is readable standing still and unreadable in motion. A list
opens *on* its current value, so the two coincide on the first frame; walk away
from that row and the fill left behind looks exactly like a second candidate,
which is the one thing a list of choices must never show. A mark cannot be
confused with a fill however the two move. The tick sits at the row's end and
not in the completion list, and the objection that once removed it no longer
holds: it used to pull a *centred* label off centre, and a choice row's content
is now pinned to the start by a flexible column of its own.

The fill is the theme's selected fill with the ink that goes on it — the one
spelling a selected thing takes everywhere in this window. It is not the loudest
fill the theme has: that one is spent on the single most important action on a
screen, which a row in a menu is not.

The pointer moves the highlight through a **mouse-move handler and not a hover
style**, because the component library gives a button no hook to set its own
hover fill, and a second fill derived from a different token an inch below the
first is exactly the drift this list was flattened to avoid.

**Rows read from the left**, all of them and not only the ones carrying a path.
A centred column of choices gives the eye a different starting point on every
line, which is the one thing a list is meant to spare it. The component library
centres a button's content on a box no call site can reach, so what un-centres
these is giving each row something that takes the leftover width — the detail
column where there is one, and otherwise nothing at all, which is exactly the
point.

**Anything clickable is keyboard reachable.** Conversation-title menus,
composer selectors, completion candidates, question tabs and transcript
disclosures use the shared button primitive rather than a clickable `div`, so
Tab traversal, Enter/Space activation, focus presentation and accessibility
roles stay one behaviour across the pane.

**The prompt field keeps the caret through all of it.** Opening a selector,
taking a row, inserting a trigger from the toolbar — each is a click on a plain
surface, which lets the pane take focus, and the keys that walk a list only
reach it while focus is inside the composer. So every one of them puts the caret
back in the field, which is also where the next thing the user types belongs.

**Every overlay is the same card, in the same place** — above the composer, the
width of the reading column, whether it is completion, a selector or the
attachment manager. The option lists used to hang off the chip that opened them,
on the reasoning that a compact surface against its trigger says which control
it belongs to. What that cost is the thing a list of choices is for: sized to
its own rows and pinned to one end of the card, a model list had no room for the
sentence the agent sends about each choice, and the rows it did fit were
narrower than the words in them. The chip stays lit for as long as its list is
open, which is what actually says where the list came from.

**A list is capped by the panel, not by a number.** A popup shorter than the
room it has draws whole and does not scroll — a list that scrolled with four
choices in it hid the fourth behind a gesture nobody needed to make. The cap is
for the other end: a list must not grow past the panel, where its top rows would
be drawn over the header or off the window with nothing on screen saying so. So
the height is the well the transcript occupies, less what the composer and its
rest already stand in, less the room a card leaves above itself. A fixed cap was
a guess about a panel that is dragged — most of a short pane and a third of a
tall one, so the same list scrolled on a maximized window with space to spare
beneath it. A panel too short for any of that bottoms out at a floor and
scrolls, which is honest; it is never reduced to one row and a scrollbar.

**A popup never moves the conversation.** It is transient chrome: it may cover
the transcript, but it sits outside the box the transcript's bottom clearance is
measured from. Measured, that clearance would grow by the popup's height the
moment one opened and shrink again when it closed — so every `@` typed would
shove the conversation up and every completion would drop it back.

**A mention is positional; a command is not.** `@` goes in at the caret, because
it names a file at the point in the sentence where it is written. A prompt is
one message and the adapter reads a command off the front of it, so `/` is a
trigger only at the very start — `src/main.rs` and `and/or` stay prose. The
toolbar's `/` therefore goes to the **front of the buffer** wherever the caret
was, and whatever is already written stays put as the command's argument;
dropped at the caret it left a stray character mid-sentence and opened nothing,
which reads as a dead button.

**A row leads with the part being looked for.** A file candidate prints its
filename first and its folder after, quietly, because a path printed whole and
truncated loses its tail — which is the filename the query was typed against. A
slash command carries the agent’s own description in the same quiet column: a
command name with nothing beside it is a name to guess at.

**An attachment can arrive three ways, and looks the same after all of them.**
The picker, a file **dropped on the card** (the card, not the tray — the tray is
not there yet the first time, and that is exactly where a first attachment
cannot be dropped), and **`Ctrl+V`** when the clipboard holds an image or a
file. A pasted image is written to a temp file first, because everything
downstream of the composer addresses an attachment by path. Clipboard *text* is
handed straight back to the input, so ordinary paste is untouched. The card’s
edge lights on drag-over with the same ink the caret lights it with, since it
answers the same question: does letting go now put the file here.

Each chip carries its **size**, because two screenshots taken a minute apart
have interchangeable names and because size is the only warning that a large
image will go as a link rather than inline. Its remove control is a **real
button**, not a bare glyph: it sits beside the name it destroys and needs the
hover and focus states that say which of the two the pointer is on. The tray is
**bounded** — a dropped folder is however many files it held — and what is over
the bound is counted, not silently dropped. That count is a **View all** control
which opens a scrollable manager where every staged item can be opened or
removed; the rendering bound never makes an attachment unmanageable.

**It is offered from the second attachment, not from the bound.** The tray
scrolls sideways, so two long names on a narrow panel already carry a chip past
the edge — and this list is the one place a chip out there can still be found and
taken off. Gated on the rendering bound instead, the way back to a staged file
nobody can see was itself invisible until there were a dozen of them, which is
the same unreachability one step later. One attachment is offered nothing,
because the single chip beside the button is already the whole list.

**A chip is also the way to the file it names**, where there is one to go to.
Three files called `main.rs` are three chips reading `main.rs`, and checking
which of them is staged means looking at it — so pressing the chip opens it in
the Workbench. An image and a file that could not be read offer no such press:
the editor reads a file as text, so both would answer with an error naming a
file the user can see is right there, and the second is already saying it cannot
be read. The pointer therefore appears over the chips that open and nowhere
else, which is the only warning a control this size can carry. Removing stays
the chip's own button, and the press that removes never also opens.

**A prompt written mid-turn is queued, not swallowed.** `Enter` or **Queue** while the agent
is working holds the prompt and clears the composer; it goes out the moment the
turn ends, opening its own turn. A **strip above the composer** says so and
carries what was written, because a prompt that has left the composer and is not
in the transcript is one nothing on screen accounts for — indistinguishable from
one the app dropped. Cancelling it puts the words *back in the composer* rather
than throwing them away, in front of anything typed since. Only a running turn
queues: nothing about the end of a turn fixes an unreadable attachment or an
adapter that is gone, so those still refuse and still say why. Stop remains
visible as the explicit danger action; Queue is never disguised as it.

**The text actions form one control row.** `+` shares a single shell with the
option chips — the same padding, radius, ink, text size, hover fill and height —
while Send remains the one primary action at the opposite edge. The chips'
labels shrink before `+` or Send can be pushed off the card.

**`+` is the one control for everything that goes into the prompt.** Attaching a
file, mentioning one and starting a slash command are three answers to one
question, and as three icons they spent the row's whole left-hand end saying it
three times. Behind a plus sign they are what the plus means, which is the only
thing a plus can mean here. Each row of that menu still **draws the mark it
stands for**, and that is load-bearing rather than decorative: an input method
can swallow a typed `@` or `/` before it reaches the composer, so these rows are
the only route to either trigger — and for somebody who cannot type the
character, the row is the only thing on screen naming it. The menu opens
**upward**, since the composer is at the foot of the window.

**A popup’s rows share one deliberate height.** They are library buttons, and a button given no size takes
the library’s own default — a step above everything in the row that opened it,
chosen by nobody, and invisible for as long as those lists were as wide as the
reading column. The rows that are a sentence *about* the list rather than a
choice in it — that it matched nothing, that it is holding some back — take it
too: one of them standing taller than its neighbours reads as a row that can be
taken.

**The composer’s control row keeps both edges stable.** `+` and Send hold their
size, with flexible space between them. **Send is the arrow
alone** — it is the one control here whose meaning never changes, so it is the
one that can afford to be a glyph. **Stop is a glyph too**, since the danger
tint says what its word said; **Queue keeps its word**, so the two standing
together mid-turn are told apart by more than colour. What `Enter` does rides in
Send’s
**tooltip** rather than in a line of its own: it is the one convention here
nothing else admits to, but it never changes, and a fixed label would spend a
narrow panel’s last inch saying so while the chips — which do change — are the
ones squeezed out.

**Two agent-advertised groups are promoted out of the list**, one to a rail and
one to a chip of its own. Both are named, and only they: the protocol promises
no ordering and every group is just a name and a list of values, so nothing in
the data says which of them is a ladder or which is worth a control on the
strip. Every other group is rows, which are correct for anything.

**Effort is a rail of segments at the foot of the list.** Its values are a
ladder — less of a thing, then more of it — and three or four words on one line
say that, where a column of rows says only that there are four of them. It sits
below the list and outside the scroll, under a rule, because it is a second
question rather than one of the choices being scrolled through. It is also the
one control here that **does not close the popup**: a row that stayed open after
being taken would leave the reader wondering whether the click landed, while a
segment lights where it was pressed and says so itself — and the next thing
somebody does with a ladder is often try the rung beside it.

**The rail is flat: only the rung in force is drawn at all.** Outlined, it was
six boxed words with one of them filled, and the boxes were the loudest thing in
a popup whose content is the list above — six borders saying "these are
controls" about a control nobody had asked a question of. Bare, the words are a
row of words and the fill is the whole of the answer, in the popup's one
spelling for *in force*, which is the rows' own. The rail letters at the rows'
size too, label included: a step smaller and it read as a footnote on the list
rather than as a setting beside it.

Which is also why it is **not built from the segmented-control component**. That
component joins *bordered* buttons into one block, handing each child the
corners of its place in the row — the two ends round outward, everything between
stays square. Right for a joined block, wrong here: with only the rung in force
drawn, a fill square on two sides reads as a rectangle laid over the words
instead of a rounded chip around one. A plain row of buttons has no such opinion
and costs less, each rung carrying its own press rather than the group reporting
an index for the row to look up.

The block is **deeper than a row of the list**, deliberately. It is a second
setting sitting under the answer to the first, and at a row's own inset it read
as one more entry that happened to have buttons in it; the rule above says they
are different questions and the air is what makes that rule look intended.

**Fast mode is a chip in the card's own row, ahead of the model.** It is read as
a qualifier of the name beside it — the two were briefly one chip for that
reason — and the model chip is the one thing on that row that truncates, so
anything standing after it would be what gets pushed off the end. The permission
mode is on the strip below instead, because what the agent may do without asking
is standing state about the project and the session rather than about this
message. The two are the same control drawn the same way, for the same reason
either is promoted out of the list at all: the value on screen, the choices one
press away.

**A picker and not a switch**, although the setting is on or off. Two rows name
both values and tick the one in force, where a switch shows a position and
leaves the reader to work out which way round it is. It also gives the agent's
own sentence about each value somewhere to go — and on this setting that
sentence carries the reason it will not stay on, which a switch that flicked
back had nowhere to say.

**A group is dropped from the list on exactly the condition its own control is
drawn on, never on its name.** An effort group offering one rung, or a fast
group offering nothing to pick, gets no promoted control — and would be
reachable from nowhere if the name alone had removed it. One function answers it
for the list and for the chip that opens the list, because the two disagreeing
is a chip whose popup is empty.

**The keyboard does not reach the rail.** The arrows walk the list and `Enter`
takes a row, both counting in an index the rail is not part of. The chips are
ordinary pickers and the keys reach their lists exactly as they reach the
model's.

**The Model popup exposes all config choices at once.** Every agent-advertised
group — `Model`, `Effort`, or another the agent defines — is in the one list, so
the complete configuration is visible without drilling into a second screen.
Choices still carry their stable protocol group ids when applied; the agent may
re-advertise or reorder configuration while the popup is open.

**The group is said once, in a heading, and never again on the rows.** Carried
per row it was the widest thing in the list and the only part of it that never
varied, so a reader scanning for a model name read `Model · ` five times to
reach the five words that differed. The heading is drawn the same way the rows
that are a sentence *about* the list are, because it is one.

It is **not an entry in the list.** The index into that list is what the arrow
keys walk and what `Enter` takes, and a heading sitting in it is a stop on that
walk that cannot be committed to anything. It rides inside the box of the row it
introduces — which also means scrolling to a row brings its heading along.

**A choice is two lines; a completion candidate is one.** A choice carries the
agent's own sentence about it underneath, at the quieter size, because that
sentence is the only thing telling two model names apart for a reader who has
not read the vendor's notes — and set beside the name instead it either pushes
the name off the row or truncates to the three words every description opens
with. A candidate's second part is its *folder*, which is where the name is
rather than something about it, so it stays on the line: stacked it would double
the height of a list whose whole job is to put fifty paths in front of somebody
who is typing. A choice the agent sent no sentence for is one line and stands at
exactly the height every other one-line row in the popup does.

At the top, the transcript disappears at the header's rule. At the bottom it
continues behind the transparent overlay wrapper and is covered only where an
actual card occupies space. The clearance that lets the final row rest above
the composer lives **inside the scroll**, so it remains reachable without
turning the overlay into an opaque footer.

There is no full-width bottom surface. The centred composer card carries its
own background while the area around it remains transparent. Inside the shared
column, the transcript keeps left and right padding equal to the composer's
visible corner radius, preserving the same spacing rhythm within their shared
outer width.

The composer, the jump-to-latest pill and the completion popup are floating
controls, so they take the opaque `popover` surface and a clear elevation
shadow. Transcript content may continue scrolling behind their bounds, but it
must never show through **or visually merge with them** — and the second half is
the harder one, because a shadow only reads on a light canvas. On light,
`popover` is the surface itself and the shadow does the separating; on dark it
is a step above the surface, because there the shadow separates nothing. The
overlay area outside those controls stays transparent.

**Two floating surfaces meeting is the case that step cannot cover.** The option
lists open from buttons *inside the composer*, so a popup lands on a card that
is floating too and takes the very same colour — the step between them is zero,
the shadow has nothing to fall on, and the panel reads as having no background
at all rather than as a panel over another one. Where that happens the **edge**
is the only thing left that can say where one surface ends, so those popups draw
theirs a real step up instead of at hairline strength. Nothing else in the pane
needs this, because nothing else opens over a surface of its own colour.

**And a popup anchored inside a card is painted late.** A box paints its
background, then what is inside it, and then its *border* — the border over its
own contents. So the composer's outline was being drawn straight across a list
opened from a button within it, which reads as the list being see-through when
it is nothing of the kind, and no surface colour can answer it because the line
arrives afterwards. The list keeps its place beside the button and only its
painting moves, to after every box containing it has finished. Any control that
opens over the edge of the card it lives in owes the same.

- **User prompts** take the row's full width and place the bubble at its right
  end. The bubble itself is bounded well short of the column, so it stays
  legible as a question rather than becoming a second column of prose.
- **Everything agent-side** — answers, thoughts, tools, permissions, notices,
  activity summaries and their expanded members — starts on the same left axis.
  There is no speaker avatar or reserved gutter; the active agent is already
  named by the panel header and rail.
- **Space belongs to the boundary between two blocks, not to either block.** Each
  run carries the gap *above* it, chosen from the pair it forms with the run
  before it — so every boundary is described by exactly one number, and no block
  can be given a different gap above than below by whatever happened to land
  next to it. Three boundaries:
  - **A turn boundary** — a prompt on either side of it — takes the largest gap.
    It is the same number above and below, because above it opens the turn and
    below it separates the question from its answer; two sides of one space that
    differ by a step read as a turn sitting slightly low rather than as a
    decision. At the gap blocks *within* a turn take, the first row under a
    prompt reads as one more line of the question.
  - **Two index rows** — a collapsed activity or an unopened settled tool on
    both sides — close ranks at the smallest gap, because they are read as one
    list. An index row beside anything else does not: an answer pulled up to a
    folded strip reads as part of it.
  - **Everything else** takes the ordinary block gap.
  Owning the gap *below* instead is what produced both faults: a block could only
  say how much room it wanted after itself, so a prompt sat further below prose
  than below a folded strip while always giving one fixed gap to its own answer,
  and a folded strip glued the next answer to itself.
- **A run has two ends, and an opened group is not the same kind at both.** The
  gap above a run answers to what it *begins* with, the gap below to what it
  *ends* with. An opened activity run ends as a block's worth of reading, so
  what follows it takes a block gap — but it still begins with the same index row
  it began with while closed, and nothing about the boundary above that row
  changed. Read from the run as a whole, opening a group tripled the space over
  its own header: the row slid down under the pointer that had just clicked it,
  and everything above appeared to shift for a reason nothing on screen gave.
- **What a group opens into is separated by rules, not by a cadence.** Its
  members are the group's own body rather than a list that happens to sit under
  its heading, so what stands between one and the next is a hairline and no gap
  at all. Out in the transcript the same row sits at the tightest gap from its
  neighbours; inside a group it sits against them.
- **The space between the paragraphs of one answer is bounded by the space
  between whole blocks.** The markdown renderer's own default is wider than the
  gap the transcript sets between an answer and the tool card beneath it, which
  makes the inside of a turn louder than the transcript's rhythm — so it is set
  down to match rather than left alone.
- **Expanded activity has one level of hierarchy.** A group header remains on
  the transcript axis with prose; its expanded members move in by one icon
  column. A leaf's detail card moves in once more past that leaf's icon so its
  edge aligns with the activity label it belongs to.
- The list is virtualized (`gpui::list`): rows are drawn on demand, so the plan
  the list reads has to be owned state rather than a borrow from the frame that
  built it.

---

## 2 — Typography

**The transcript has two voices, and size is what tells them apart: what was
said, and how it got made.** An answer, a prompt and a thought's reasoning are
the first. An activity row, a plan, a group header and every descriptor on them
are the second — they are the record of the work, not the work, and at the
answer's size they compete with the thing the reader came for. The two blocking
cards are the deliberate exception: a permission and a question are the only
blocks where nothing at all proceeds until the user acts, so they speak at the
conversation's size — though the sentence *explaining* one of their options is
not the option, and takes the second voice like everything else that describes.

**The step between the voices is defined against the reading size, not against
the app's base.** Written as a fixed step under the base it survived exactly
until the reading size moved down to meet it, at which point an answer and the
tool card beside it came out identical and the distinction this section is about
stopped existing. The second voice lands on the same number as the wells of
machine text and stays a separate decision from it: a card's header is chrome
around output rather than output, and nothing else marks the two alike — a well
is mono, tinted and padded, and a header is none of those. Below both, the quiet
disclosure rows sit one step further down again; between three tiers inside a
few pixels, size is what orders them and weight, ink and shape are what make
each legible on its own.

**The inherited size is the transcript's own, one step under the app's base.**
A conversation is read the way a page is: it is long, it is mostly prose, and
the eye travels down it rather than stopping at each field. At the base size —
chosen for labels and controls that have to be hit — a long answer is a wall.
The step down is set once, on the frame every run is drawn in, so prose, cards,
wells and rows all take it together and the *controls* around the transcript
keep reading as the larger things they are. The markdown renderer's heading base
takes it too: headings scaled off the app's base over a body a step under it
would print a third-level heading larger than the prose it names, for no reason
the reader can see.

| Role | Written as |
|------|------------|
| Prose, messages, thought bodies | the inherited size — nothing set |
| A blocking card's question, and its choices | the inherited size |
| Tool cards, plans, and a choice's explanation | one step under the reading size |
| Every activity row — a step, a run, a thought, a settled exchange, and their summaries | `text_xs` |
| Attention tool names and "Plan" | semibold, at their card's size |
| Such a row's *verb* — `Inspected`, `Explored`, `Reasoned` | semibold, at `text_xs` |
| Such a row's *summary* or descriptor | regular + `muted_foreground`, at `text_xs` |
| Meta — tags, status, timings, counts, attachment rows | `text_xs` + `muted_foreground` |
| Code, diffs, terminal, `IN`/`OUT` bodies, fenced blocks | the theme's mono family, one size, leading tightened from the prose default |

**Every well of machine text is one well.** A tool's output, a diff, a live
terminal and a fenced block quoted inside an answer are the same claim — a
machine produced this — so they share a size, a padding, a tint and a leading.
The last of them arrives through a different renderer and has to be given those
values explicitly; left to itself it draws at a pixel size of its own, and the
same command reads at one size in a tool card and another when quoted back in
prose.

**Prose leading is wrong for a diff.** The golden ratio is right for a
paragraph and wrong for two hundred lines each carrying two thirds of a blank
line — a column of half-empty rows the eye cannot track down.

**Headings inside an answer are section marks, not a document title.** The
markdown renderer scales them off a base of its own, given in pixels: left
alone, `#` prints as a document title inside a chat message, `####` and below
print *smaller* than the paragraph they name, and none of them move when the
panel is zoomed. The base is therefore handed in from the rem size in force at
render time, and only the first two levels step up — past that, weight carries
the hierarchy.

**Mono is a claim that this could be pasted back into a machine**, which is what
decides the two edge cases. A tool's descriptor is mono only when the descriptor
*is* the command — an `Execute` step that arrived with a description is being
described in prose, and prose set in mono lies about what it is. A section's
`IN`/`OUT`/`EDIT` tag is meta, not content, and stays in the body face beside a
mono body.

**Inline code inside prose is *(not rendered)* as mono.** Blocked on the
library: the markdown renderer styles inline code through gpui's
`HighlightStyle`, which carries colour, weight, slant and background and has no
font family. Making it mono would mean extending or replacing the inline
renderer. It therefore substitutes **one** channel and not two — the theme's
`blue`, no visible background, and the body weight. Weight is
available and deliberately unused: a sentence naming five symbols comes out
patched with semibold runs that read as the markdown's own bold, which is a
distinction prose actually uses.

The hue is **tempered before it is used as ink**, and toward the meta ink rather
than toward the foreground. A status colour is pulled toward the foreground
because it arrives as a fill and has to be made legible; this one is legible
already and its problem is glare — a near-saturated blue beside neutral grey
prose on a near-black surface vibrates, and pulls the eye off the sentence it
belongs to. A neutral of about its own lightness takes a third of the saturation
out and leaves the contrast where it was, which is the axis the trouble is
actually on. Derived, so the light palette tempers its own darker hue by the
same rule and neither is tuned by hand.

Sizes are rems, never pixels — per-panel zoom overrides the rem base for the
pane's subtree, and a hand-written pixel size is exactly what refuses to scale
with it (DESIGN.md §2).

---

## 3 — Colour

There is no palette in this document. What the transcript needs from the theme,
by meaning:

| Meaning | Token |
|---------|-------|
| The reading surface | `background` |
| Wells (code, diff, terminal, reasoning) | `muted`, one step in from the surface |
| Every hairline, every card border | `border` |
| Prose | `foreground` |
| Meta, descriptors and summaries | `muted_foreground` |
| Added diff lines, completed status | adaptive `status_ink().success` |
| Removed diff lines, failures, errors | adaptive `status_ink().danger` |
| The ground under an error notice | a wash of the `danger` token |
| Running, pending, "needs attention" | adaptive `status_ink().warning` |
| The fill of a control that floats over the transcript | `popover` |
| The one item selected among several | `accent` / `accent_foreground` |
| Hover on a row that is there to be picked | `list_hover` |
| The single primary action of a blocking card | `primary` |
| Marking a find hit | `list_hover` on every matching item; a stronger accent wash on the current item |

**Prose ink is not white on a dark surface.** There the ink is the bright thing
in the room, and near-white on near-black runs about four times the contrast a
body of text needs — enough to leave an afterimage on a long conversation read
in a dark room. It steps down to a soft grey that is still past AAA on every
surface it lands on, and the ramp's *relative* order is untouched: meta ink
stays quieter than prose, prose stays quieter than the ink on a selected row,
and the user's own bubble sits one step brighter than prose because its fill
sits one step above the surface. The light palette keeps its ink at full
strength: what glares there is the surface, not the text, so dimming the text
buys no comfort and spends legibility to do it.

Elsewhere in this document, `success`, `warning`, and `danger` name semantic
roles. Text and icons resolve those roles through `status_ink()`; the raw theme
tokens are reserved for fills and borders.

**The surfaces in that table are distinct values, and the app owns them for that
reason.** A transcript puts the reading surface, a sunk well, a filled bubble
and a raised floating card on screen at the same moment, with every pair of them
adjacent — and the component library's neutral ramp does not have that many
steps. `status_ink()` is adaptive for a different reason that outlives any
palette: the status tokens are *fills* with paired foregrounds, so using one as
coloured text is wrong by construction, not by accident. How far its hues are
pulled is set by the **well**, not by the reading surface, since that is where
most status text is drawn.

**Nothing is ringed; a state is a fill.** Wherever the highlight is the only
thing saying which of several items is live — a completion candidate committed
with Enter, the open tab of a multi-question card, a selector whose popup is
showing — the selected fill carries it alone, with no rule around it. That is
possible because the ramp puts it a clear stage past hover rather than a wash's
distance off the surface, and the two are asserted against each other: a row
that is hovered *and* selected must not read as merely hovered. `list_active`
is not used for this, and cannot be — the library clamps it to a fifth of its
opacity — so the library's own list highlight is switched off to keep one
answer to what a selection looks like.

**Cards are borders, not fills.** The user message is the only filled block; tool,
code and thought cards are a hairline over the surface, and their *bodies* may
sink one step. Depth is a border and padding, never a lighter box inside a
lighter box.

**Code differs from prose by font, not colour.**

---

## 4 — Collapsible blocks

One interaction with one shape. Every activity — a step, a run of steps, a
thought, a settled question, a settled grant — is a row of the same anatomy
inside the same frame, and what a row is doing changes only its state column and
the pill at its end.

```
┌────────────────────────────────────────────┐
│ ✓ «k» Inspected  chat/pane.rs ………………    ▸ │  one step
├────────────────────────────────────────────┤
│ ✓ «k» Explored   3 files · 1 search ……   ▾ │  a run of steps
│      ✓ «k» Inspected  a.rs ……………………    ▸ │    its children
│      ✓ «k» Searched   BuildSequences …  ▸ │
├────────────────────────────────────────────┤
│ ◌ «k» Ran        cargo test ………  running ▸ │  still going
├────────────────────────────────────────────┤
│ ⊘ «k» D̶e̶n̶i̶e̶d̶    r̶m̶ ̶-̶r̶f̶ ̶…̶   [allow once] ▸ │  a settled grant
└────────────────────────────────────────────┘
```

- **Activity block:** every stretch of adjacent activity between two agent
  paragraphs, inside **one** frame — a step, a run of steps, a thought, a
  settled question, a settled grant. Rows are separated by hairlines with **no
  gap**; a gap between two bordered things is a seam, while a rule between two
  unbordered ones is a table, which is what a column of rows sharing six columns
  is. **Two activity frames never stand next to each other**: the block is
  assembled from the runs it spans, the first opening its corners, the last
  closing them, and every run between drawing only the hairline under itself.
- **Activity row:** one anatomy for everything in that block, at a fixed height.
  Left to right: **state** in a fixed slot · **kind**, the block's drawing for
  the sort of work · **verb**, what was done, the one part at full ink ·
  **object**, what it was done to, muted and cut to one line · **meta**, words
  and numbers against a right-hand floor · **chevron**, always the last column.
  Only the object gives way when the row runs short. A row with nothing to open
  draws no chevron and takes no pointer.
- **A row that stands for a run of reads and a row that is one step are the same
  row** collapsed. The only difference is what each opens into: one unfolds a
  command and its output, the other the paths it stands for.
- **A row hovers the way the cluster's line above it does: ink, and no plate.**
  A fill behind a row is the row answering as a surface, and these rows are a
  list inside a frame that is already one. The **weight is left alone here and
  only here**: the verb is a shrink-to-fit column, so a heavier one moves where
  the object column starts, and a block of rows whose columns shift under the
  pointer is the thing the frame exists to prevent. The layout and the ink sit
  on the row element itself rather than on a box inside it, because a hover
  styles the element whose hitbox the pointer is over and text colour cascades
  *down*: a wrapper hovering over a child that has already set its own colour
  changes nothing. Everything that should lift inherits; the verb, already as
  bright as the row goes, says so.
- **Every mark on a row is dropped a pixel onto the line its words read on.** A
  centred box and centred *type* are not the same place: the renderer puts a
  line's baseline at `(line_height − ascent − descent) / 2 + ascent`, and since a
  face's ascent is the larger of the two the baseline lands below the middle of
  the box — so lowercase text sits about a tenth of an em low inside its own
  line, and a glyph centred against that box comes out looking that much high.
  It is the one measurement here that is not on the spacing scale, because it is
  an optical correction and not a space; one function owns it, since a row has
  two or three marks and the first one added without it reads as the row having
  come apart.
- **Motion:** none. The chevron swaps rather than rotates; body height is not
  animated. A transcript that reflows while it streams is harder to read, not
  livelier.
- **Plan:** the one framed block that is not activity. Same frame, same corner, a
  heading carrying an exact count and a bar under it carrying the same figure as
  a length, then one entry per row at the control height with a checkbox that
  never changes size — only what is inside it.

---

## 5 — Block types

The `ChatItem` variants — `User · Agent · Thought · Tool · Plan · Permission ·
Ask · Notice` — plus the activity block and the chrome rows (§6).

### 5.1 User prompt
The **only** block with a fill, and the only one on the right.

- Body: filled, rounded, no border, shrink-to-fit against the right edge and
  bounded to a fraction of the row before it wraps. A one-line question
  stretched edge to edge is shaped exactly like an answer, and the shape is
  what the eye reads first.
- Attachments stack **above** the bubble and **outside** it, on the same right
  edge, each a bounded thumbnail for an image (§12) with a quiet caption under
  it — kind icon, file name, and a `danger` "not sent" mark on anything the
  agent never received. What was handed over is not what was typed: inside the
  fill a picture reads as part of the sentence and is bounded by the sentence's
  box, and a prompt that was nothing but a screenshot drew an empty filled card
  above it. Bounded, with the rest counted. **Named, not counted**: "3
  attachment(s)" cannot be checked against what the user meant to send, so the
  one mistake it hides — the wrong screenshot — reads as correct until the
  answer is about the wrong picture.
- The bubble itself is drawn only when something was typed, so an
  attachment-only prompt is the files alone.
- *(Not rendered: the per-message footer with Copy / Select text, and the
  long-prompt clamp with "Show full message".)*

### 5.2 Agent answer
Markdown prose starts directly on the transcript's shared left axis. There is
no avatar and no empty speaker gutter; the panel header and rail already name
the active agent, while the user bubble's right edge distinguishes the other
speaker.

- **Prose** is gpui-component's `TextView` over the model's markdown source. The
  parsed form is cached per block and grown by *appending* the new bytes rather
  than re-parsing the whole source per token — re-parsing is what makes a long
  answer slow down as it arrives.
- **Code blocks inside prose are not §4 cards, and do not fold.** They are
  gpui-component's code-block renderer, styled to a bounded height with Copy
  offered on each. `TextViewStyle::code_block` is one style for *every* block, so
  per-block fold state has nowhere to live; reaching it would mean replacing that
  renderer through a custom block parser and trading away its syntax
  highlighting to get a chevron. The cap keeps a long answer readable, which is
  what the fold was for, and Copy is how the clipped tail stays reachable. The
  model carries no fold state for a block, so there is nothing here that is
  half-wired: adding the fold means designing both halves at once.
- **The turn's chrome sits on the turn, not on the block.** An answer split by
  tool calls arrives as several `Agent` items; the label goes on the first and
  the footer — "Processed in Xs", and Copy — on the last, both decided by the
  model (`Chat::turn_answer`). Copy takes the *whole turn's* prose, and is
  absent while the turn is still arriving: a Copy offered mid-stream silently
  copies however much had landed by the click.

### 5.3 Thought
Collapsed reasoning, never containing tool calls. One activity row —
`Reasoned · Xs`, or `Reasoning` with a spinner while it is still arriving —
collapsed by default, its body unfolding under it at the verb's own inset. The
two states are the same row: the mark and the verb change and nothing moves.

### 5.4 Activity cluster
Everything the agent did between two of its own paragraphs — or between a prompt
and its first paragraph — is **one cluster**, and a cluster is **one muted line**.

- **Bounded by the agent's words and by nothing else.** Not by the kind of work:
  three reads and a command between one paragraph and the next used to draw two
  headers, which is two claims about one stretch of work with nothing between
  them to explain the seam. Not by status either: a running step used to sit
  outside the cluster and move in when it finished, so the row count changed
  every few seconds mid-turn and a step that had been a card became a line in a
  list somebody was already reading. **A cluster of one is still a cluster** —
  the exception meant the transcript had two ways of saying the same thing, and
  which one a reader got depended on whether the agent happened to do a second
  thing afterwards. Settled questions and settled grants are in it too.
- **Two clusters never stand next to each other.** If nothing of the agent's is
  between them, they are one cluster.
- **The line has no frame, no fill and no rule.** It sits on the reading surface
  at the same left edge as the prose either side of it, in the ink a marginal
  note is set in, and it **shrinks to what it says** rather than ruling a bar
  across the column — past the column it truncates. Shrinking is load-bearing
  and not decoration: the line is a library `Button`, which centres its own
  content with no way out, so stretched across the column the sentence sat down
  the middle with the prose either side of it starting at the left edge. A
  column flex stretches its children by default, so the line's container has to
  say otherwise. Its columns: the sentence, an error count, the lines added and
  removed — mono, each side in the ink it means, and **a side that is zero is
  not drawn**, which colour is what forces: `−0` set in the danger ink is the
  colour of something having gone when nothing did — then how long the whole
  stretch took, and **the chevron, last — the same end of the row it takes
  inside the frame**, so the one control meaning the same thing everywhere is
  not in two places depending on which kind of row it is on. The total is a sum of durations each step stamped
  once when it settled, not a clock: a live figure is a number the line re-reads
  every frame and never comes to rest on. It is drawn only where something
  actually reported one, or a cluster whose steps never said would claim to have
  taken no time at all. The whole line is the control.
- **Hovering is the ink and the weight, and no fill at all** — the line's own two
  channels turned up rather than a plate put behind it, which is what a note in
  the margin has to do: a rectangle appearing between two paragraphs every time
  the pointer crosses the column is the chrome answering instead of the thing
  hovered. With no plate there is nothing for padding to hold the text off, so
  the sentence starts exactly where the prose does.

  **The line is a stateful `div`, not the app's button wrapper**, and that is
  what makes the hover land. The wrapper is a library `Button`, and reaching its
  hover state from a call site means going through three layers — the button's
  own refinement, the `Stateful<Div>` underneath it, and the group-hitbox
  registry a `group_hover` resolves against. Two attempts at that changed
  nothing on screen. `hover` on a stateful div is the primitive all three are
  built out of: it styles the element whose own hitbox the pointer is over, with
  nothing in between to go wrong. What it costs is the keyboard, which a
  `Button` would have carried; the rail's rows made the same trade.

  What it costs is that the line is shrink-to-fit, so a heavier weight makes it a
  little wider — the right-hand end moves under the pointer while every word
  before it stays put. Taken deliberately: the alternative is a plate, and the
  wobble is at the end of a line nothing is aligned to.
- **At the transcript's own reading size**, and never more. It has been all
  three: a step under it — where it started — read as a footnote to the paragraph
  above rather than as the heading of what came next; a step over it made the
  line the only thing in the transcript set larger than the agent's own words,
  and size is loud in a way ink is not, so it out-measured every answer it sat
  between. At the reading size it is neither. It takes its place in the column
  and lets the light weight and the muted ink say how much of the reader it
  wants, which is what those two channels are for.
- **Muted end to end, lighter than the prose, placed by size alone.** It has
  been through a status mark, a brighter ink on its verbs and a heavier weight —
  each added so the line could be found while skimming, and each one also making
  it compete with the answer above it, which nothing here may do. Size is the
  channel left: it says where a reader is in the document without saying how much
  the thing wants from them, and the light weight is what keeps a larger size
  from reading as a louder one. A weight is a request like a family is: it lands
  only where the resolved face carries that cut, and asked for something far
  enough off the platform may answer with a different family altogether — which
  would put this one line in a typeface of its own. So nothing depends on it:
  size and ink carry the line, and the weight is the third channel. There
  is no status glyph — what went wrong is a count, said in words and in the ink
  that means it.
- **The sentence is kinds of work, in the order they first happened, each with a
  count**: `Read 3 files, ran 7 commands, edited 2 files, 3 other steps`. Not one
  phrase per step, which costs as much to scan folded as unfolded; not a bare
  total, which says how much happened without saying what. Only the opening word
  is capitalised. The agent's own housekeeping — looking a tool up, stopping a
  task — has no verb and goes last, because given one it reads as work on the
  project. A settled question reads `asked n questions`; a grant is not
  mentioned, being the user answering rather than the agent working. **No command,
  no long path and no secret ever reaches this line.**
- **While something is running the sentence leads with it, in the present
  tense**: `Running dotnet test · read 2 files`. A line opening with what is
  finished buries the one part of it still changing. The step in flight is not
  counted among the things that have been done. When it settles the line changes
  in place; its height, its position and its cap do not.
- **How the cluster went is said in words, not in a glyph.** How many failed sits
  beside the sentence in the danger ink, as a number rather than a state — so not
  a pill; and a cluster still working leads its sentence with what it is doing.
  (The *rows inside* it still carry marks, and those read the section's ending
  rather than its worst moment: quiet, `warning` where a failure was recovered
  from, `danger` where the section ended on one.)

  **Those marks are a square in the state's own ink, not a glyph naming it.** A
  tick and a cross are two drawings to read at a size where both are a handful
  of strokes, and a column of them down a block is a column of small pictures
  competing with the words beside them. A square is one shape wherever it
  appears, so what the column carries is a colour — and a colour is read without
  being looked at. The mark is sized off the glyph it replaced rather than
  fixed, so a parent's and a child's stay the step apart that says which level a
  row is on. Running keeps the spinner: a static dot cannot say *moving*.

  **Nothing critical rests on the colour alone**, which is the rule this would
  otherwise break. A failed row still prints `failed` in its right-hand column,
  and the cluster's own line still says `· n errors` in words — so the square is
  the fast channel and never the only one.
- **Collapsed by default, always** — including a cluster with errors in it and a
  cluster still running. One that opened itself would push the answer above it up
  the panel every time a turn started work and shut again when it stopped. The
  fold is the user's and survives the transcript growing.
- **The frame exists only once it is asked for.** Drawn always, it was a box the
  height of a paragraph standing between two paragraphs, for steps nobody had
  asked to see — the detail claiming the space of the answer. Opened, it appears
  under the line at the tight gap, takes the full column, and is the single frame
  layer: **one border, the block corner, and no fill**. A fill would put a second
  surface inside the reading one — a slab of another colour standing between two
  paragraphs for as long as it is open — and it buys nothing the border does not
  already say. It also leaves the rows inside their full hover contrast, which
  against a surface already a step off the reading one was halved. Everything
  inside separates by hairline, never by another bordered box.
- The line sits a block's gap from the prose above and below it, and when open
  that gap is measured from the foot of the frame.

### 5.5 Activity row
One anatomy for every kind of step inside an opened cluster, columns spaced
evenly and aligned down the whole block:

| Column | Size | Holds |
|---|---|---|
| State | fixed slot | a disc in the ink the state means — quiet waiting, `success` done, `warning` recovered, `danger` failed, quiet again refused — and a spinner while it runs |
| Kind | fixed slot | the block's drawing for the sort of work |
| Verb | its own width | `Read` · `Edited` · `Ran tests` · `Asked` · `Allowed`, in the reading face and the reading ink |
| Object | takes what is left | what it was done to, in the machine face a step under, one line, ellipsized |
| Meta | right-aligned | `+N` / `−N`, `exit 101`, `failed`, `deleted` — words and numbers, never a plate |
| Chevron | fixed slot | the arrow, reserved whether or not it is drawn |

- **The exit status where there is one, the word where there is not.** Only a
  command run through the terminal extension reports a code; an adapter that
  says a step failed as a plain tool call has none to give, and one invented
  from its output is a fact the reader would then act on.
- **The state is a disc, not a glyph.** A tick and a cross are two drawings to
  read at a size where both are a handful of strokes, and a column of them is a
  column of small pictures competing with the words beside them. A disc is one
  shape wherever it appears, so what the column carries is a colour — and a
  colour is read without being looked at. Running keeps the spinner: a static
  shape cannot say *moving*. **Nothing critical rests on the colour alone** — a
  failed row still prints `failed`, and the cluster's line still counts errors in
  words.
- **A path arrives split.** A reader scanning a column of them is looking for the
  *name*; the directory above it is only there for the times two names are the
  same, and at one weight it takes the eye first every time it is long. So the
  directory recedes a step and the name keeps the reading ink. A command is not a
  path and is never cut at its last slash. A deleted file is struck through.
- **Only reads merge.** Three files looked at in a row are one thing the agent
  did, and `Read 3 files` opening into the three paths is what a reader wants of
  them. Two edits are not: each carries its own diff, which is the thing somebody
  opened the block to see, and folding them behind one row puts it two clicks
  away to save a line. The same for two commands, whose output is the point of
  each.
- **A failure opens itself, and can still be shut.** The one thing a reader needs
  from a settled step is whether it worked; for the one that did not, the next
  question is always what it said, so making them ask is a click charged for the
  case that already went badly. It is **seeded into the fold the user owns**, not
  OR-ed into the open rule — a terminal status forcing the row open forces it
  open *for ever*, so the control that shuts it does nothing and the one state
  that most wants a way out is the one with none. Running gets away with the OR
  because it stops being true on its own.

### 5.5.2 What a row opens into
Set in to the row's own words, in one box: a border, the block corner, and a
single step off the frame — a layer, not a card dropped in. Machine text
throughout, at a leading a list of it wants.

- **A command, then what it printed**, separated by a hairline. The `$` says
  which is which; there are no `IN` / `OUT` labels, which were four characters of
  chrome per section and a fixed column taken off the widest text in the
  transcript. The ink follows the *line*, not the row: an error in the middle of
  a hundred quiet lines is the one somebody is looking for.
- **A diff in three columns** — number, sign, text — the number and sign pinned
  to the *first* row of a line that wraps, or the column down the side stops
  being a ruler the moment anything is long. Added and removed rows take a wash
  of their own ink, never a solid fill. An elided run is its own line, in the ink
  that means there is more behind it.
- **A diff past four hundred changed lines is offered, not drawn.** One that size
  is searched rather than read, and every line of it is an element in a list
  already virtualising rows for the same reason.
- **Collapsed shows the point and fades the rest**: a diff from its top, because
  that is where the change is; a command's output from its bottom, because that
  is where its failure is. The cut fades into the box at whichever end it falls,
  and the control that opens it is a pill sitting over the fade — a plate of its
  own, or it is read against the text it is covering.
- **Opened, the box scrolls inside a fixed height**, and the control that shuts
  it sits *outside* that scroll with a rule between, so it never goes where the
  content goes. **Collapsed it does not scroll at all** — a preview short enough
  to read whole has nothing to scroll, and a box that scrolled anyway would be a
  second scroller under the reader's finger for no reason.
- **The wheel has to be taken in the capture phase.** The transcript is a
  `gpui::list`, which registers its own wheel listener before its children have
  any say, so a box that merely sets `overflow-y: scroll` is a box the wheel
  slides the *conversation* behind: the reader ends up somewhere else in the turn
  while trying to read one command's output. A mask sits as a sibling of the
  scrolled box and consumes the vertical delta before the list sees it.
- **A box with nothing to scroll takes nothing**, which is the half a first
  attempt forgets. Consumed wherever it is drawn, an opened detail shorter than
  its own cap becomes a dead patch of the transcript: hovering it stops the
  conversation moving, for a box that was not going to move either. The mask
  reads last frame's travel and stands aside when there is none.
- **Where there is travel it is contained, and not chained.** It keeps consuming
  at the edge rather than handing the delta back, which is the opposite of what
  a browser does by default and is right here: these boxes are a few lines tall inside a
  transcript that is hundreds, so a reader who reaches the end of one command's
  output would have the whole conversation take off under their finger. What
  they were doing was reading *this*, and arriving at its last line is not a
  request to leave it. The component library's own mask chains, so the
  transcript brings its own.

### 5.5.1 The record a settled exchange leaves
A question the agent asked, and a grant the user answered, are **rows of the
block** once they are settled — not cards.

- While either is waiting it is not in the transcript at all: it is pinned above
  the composer, where the answer is given. What is left afterwards is the fact —
  the agent asked this, the user said that — and that is one line of the same
  list a step is a line of. Drawn as a card it was the loudest thing in the
  conversation for the rest of the conversation's life: a heading, a rule, a
  body, a footer and a shadow per exchange, so three questions in a row cost
  most of a screen to say three sentences.
- The **kind slot carries the user**, because every other row in the block is
  something the agent did and this is the one where somebody answered.
- A **grant** reads `Allowed` or `Denied`, with the command as its summary and
  the option's own wording as its pill. A refusal is not a failure — nothing
  went wrong, a decision was taken — so it takes neither the tick nor the danger
  cross, and says what happened by striking the words out. Opened, it gives the
  whole command and the directory it would have run in.
- A **question** reads `Asked`. One question puts the question in the summary
  and the answer in the pill, and has nothing to open. Several put the agent's
  own prompt in the summary and the count in the pill, and open into one row per
  question with its answer right-aligned beside it — **no rules between those
  pairs**: they are one answer given in several parts, and ruled apart they read
  as separate exchanges. A typed answer takes the same pill as a chosen one;
  which it was is carried by weight, not by a different control.

### 5.6 Command
The terminal icon and mono descriptor distinguish a command — **no accent
rail** (§3). While pending or running it is a standalone card; once completed
or failed it becomes the same compact row as every other settled tool and may
join adjacent settled work in one activity row.

### 5.7 Permission
A blocking card: the agent parks until it is answered.

- A `warning` icon and a short heading; the agent-authored command sits in a
  bounded mono well.
- **The well is bounded against the panel it is drawn in, not the window.** The
  bound is there so the heading stays on screen with the command it belongs to —
  a grant read without the sentence saying what is being granted is not a grant
  anybody gave — and with a dock open half the window is taller than the whole
  conversation, so a share of the window pushes *Permission required* off the top
  instead of holding it there. It is bounded whether or not it has been opened:
  the fold counts the agent's own newlines and says nothing about how tall they
  draw, so eight real lines of a base64 blob is still a screenful of wrapped
  rows. What is held back scrolls **inside the well**, which means the well takes
  the wheel before the transcript behind it does; otherwise an opened command is
  a box that slides the conversation past while the reader is trying to read it.
- **Every command is drawn the same way, whatever shape its text is.** A
  numbered gutter, a Copy button on the surface, and — wherever anything is out
  of sight — a fade and a control that opens it. Which card a permission got used
  to be decided by whether the agent happened to put a newline in its command:
  the gutter appeared past the second line, the fold past the eighth, and Copy
  waited for the pointer below that. So a long `curl` and a `bash` script asking
  for the same grant, an inch apart in one transcript, were two different-looking
  cards — and the difference was not anything the reader had done.
  **The fold slices lines; the box is bounded by height, and the two are not the
  same question.** *Which* lines are drawn is decided by the newlines the agent
  wrote, and that stays: measured in drawn rows it would close a two-line command
  on a narrow pane and leave a ten-line one open on a wide one, a fold nobody can
  predict. But that rule cannot bound one line three thousand characters long,
  which is one line to it and a screenful to the reader — so the folded box is a
  **height**, the height of the lines the fold would have shown. Every command
  folds to the same box, and the control that opens it is offered on the same
  terms: whenever anything is out of sight, by either route.
  The count rides in the label only where lines are what is being held back —
  *Show all · 200 lines* — because a single wrapped line has no second line to
  promise and *Show all · 1 lines* counts the wrong thing and miscounts it.
  The fade goes as soon as the end is reached, since a gradient laid over the last
  line of a command somebody is being asked to approve is the one place this
  cannot be decorative. **The scrollbar waits for the block to be opened**: folded,
  the way to the rest is the control at the corner, and a scrollbar beside it is a
  second and quieter answer that moves the text without ever saying how much of it
  there is.
- **The header names what kind of work is being asked for**, taken from the
  tool call's own declared kind — read, edit, execute, fetch. The card needs one
  word of that sort: a heading that is a bare command says what *would* run
  without saying that it would be run at all, and the protocol carries no tool
  name to fall back on. A kind this build does not recognise prints **nothing**
  rather than a word invented for it, because the one thing worse than an
  unlabelled grant is a mislabelled one.
- Actions split across the footer: Deny is a quiet ghost, "Always allow" a
  neutral outline, "Allow once" the one `primary` action.
- **Unanswered, it is pinned above the composer, not left in the transcript.**
  The transcript scrolls and the pin does not: a permission that arrived four
  screens ago is still the only reason nothing is happening, and hunting for it
  is not a thing to ask of someone who is already waiting. The transcript leaves
  the card out entirely while it is pinned, so it is never on screen twice.
- **`Enter` allows once and `Esc` denies — and which option either means is the
  option's own weight, never its place in the list.** An agent is free to send
  its grants in any order, so a key answering by position would grant *always*
  on a card that happened to list it first. A weight no card offers is no key at
  all, since there is nothing to send. Both are taken on the card's own handle
  rather than bound as app actions: each is a key somebody is as likely to be
  pressing in the composer an inch below.
  **`Enter` answers only while the card itself holds the caret.** Every button
  in the footer is a button, a focused one already turns `Enter` into its own
  click, and a click settles on the key going *up* — so a card that also answered
  on the way down would beat it. Somebody who has tabbed to *Deny* and pressed
  `Enter` would have granted the call: the grant lands first, and Deny's own
  click arrives afterwards to find the permission answered and is dropped. On
  this card above all others, the key that means no must not be how yes gets
  said. `Esc` needs no such guard and carries none — nothing here denies by
  being focused, and the worst it can do is refuse a call twice.
- Resolved → the controls drop, the card leaves the pin and takes the place it
  always had in the transcript, as an audit trail rather than a live control.
- Several parked cards — a permission and a question at once — pin in the order
  they were asked in, which is the only order that makes sense of them.

### 5.7.1 Question (elicitation)
The agent *asking*, which is not the same as asking permission — Claude Code's
`AskUserQuestion` arrives as an ACP form elicitation. Same neutral card shell as
§5.7.

- A single-question, single-select form answers **on click**; richer forms
  collect picks and commit on Submit. Skip declines without ending the turn.
- **A multi-question form is tabbed**, one question at a time: stacking every
  question made the card taller than the pane and pushed the top of it off
  screen. The tab strip scrolls horizontally.
- **The tabs are numbered, and the number is the point until the question has an
  answer.** A strip of titles says these are three things; a strip of *numbered*
  titles says they are three things in an order, with a first and a last — the
  only question a reader partway through a form actually has. Once a question is
  answered its number has done that job, and **a tick replaces it**: what is
  left to do is then readable as the tabs still carrying digits, without
  counting anything. The two never show together, because one circle holds one
  mark — which is the whole reason this is a replacement and not a badge added
  beside the number.
  The circle is filled at full strength for the question that is open, quietly
  for one already answered, and not at all for one still waiting, so the two
  marks are never the only thing separating the three states.
- **The open tab is underlined, not filled.** A filled tab in a strip whose tabs
  each already carry a filled circle is two fills arguing about which one means
  *here*; the rule lands on the strip's own hairline and reads as the one
  continuing into the body below.
- **The heading counts out loud** — *Question 1 of 3*, at the right-hand end —
  because the strip says which question is open but not how many are left until
  the reader has counted them. Absent on a one-question form, which has nothing
  to count.
- **A choice carries a radio or a checkbox**, round for a single-select and
  square for a multi-select. It is the one convention here inherited rather than
  invented: a reader who has met a form before knows a circle means *instead of*
  and a box means *as well as*, and nothing else on the row says it. Its **ring
  is ink, not the hairline** — the difference between a control and an edge. In
  the hairline it vanished the instant the row was hovered: a ghost row's hover
  fill is derived from the same step of the ramp the hairline sits on, so the
  two land within a shade of each other in the dark palette and the ring is
  painted onto its own background. A mark that disappears under the pointer
  disappears exactly when it is being aimed at.
- **A choice whose label says it throws work away is drawn in the danger tint.**
  The protocol carries no flag for this — a form's choices arrive as a bare enum
  of strings, and nothing on the wire separates *Leave them* from *Drop them*.
  The one place the difference is written down is the wording the agent chose
  for the person reading it, so the test is the label's own words: *delete*,
  *drop*, *remove*, *destroy*, *erase*, *wipe*, *discard*, *purge*, *overwrite*,
  matched whole rather than as substrings so *undropped* and *removable* are not
  read as warnings.
  **A heuristic is affordable here only because it decides a colour and never an
  answer.** A false positive tints a harmless option and costs a moment's
  hesitation; a false negative draws a destructive one exactly as plainly as it
  would be drawn with no rule at all. Neither can pick anything on the user's
  behalf: the choices keep the agent's own order, nothing is pre-selected on the
  strength of the test, and no row is disabled by it. That is the line — a
  heuristic may change how a row *reads*, never what a press *does*.
  Its stated limit: the words are English. An agent answering in another
  language gets no tint, which is the same as having no rule — the failure is
  silence, not a wrong colour.
- **The free-text answer is drawn as one more row of the list**, with a pencil
  at its head — because that is what it is, the answer under the last of the
  agent's. Bare, it read as a field left over from somewhere else. The row is
  the border and the field inside it draws none: two rings around one input read
  as two inputs.
- **The keyboard has a walk and a jump, and the row says which key reaches it.**
  The arrows move a cursor and settle nothing, `Enter` takes what the cursor is
  on, `Esc` passes on this question at the same scale the *Skip* beside it does
  — and a digit jumps straight to the row carrying it, the free-text box
  included. The walk is named once at the foot of the card; the jump is named on
  each row, because a jump has to be to something the reader can already see a
  name for.
  **Only the first nine rows carry one.** A keystroke is read whole and there is
  nowhere to hold a half-entered figure, so nothing past the ninth has a key —
  and a row drawn with a `10` on it is worse than one drawn with nothing: it is
  a press that cannot be made, and on the card that answers the moment a row is
  taken, reaching for it lands on `1` and commits the first choice instead. A
  tenth row is a row without a key, not a row with an unusable one. A digit
  nobody offered does nothing at all, rather than rounding to the nearest row.
  **`Enter` answers only while the card itself holds the caret.** A choice row
  is a button, and a focused button already turns `Enter` into its own click —
  so answering here as well is two answers, which on a multi-select is the
  choice toggled on and straight back off. The row that has the caret settles
  itself; this is the walk's `Enter`, for the cursor the arrows moved. The
  free-text box takes every key while it has the caret, digits first of all,
  since they are exactly what somebody writing their own answer types.
- **The primary action says which one it is.** With a later question still open
  it is *Next →* and moves the form on; on the last it is *Submit*. A Submit on
  question one of three reads as an answer thrown away early.
- **Choice labels are never elided** — an option the user cannot read whole is
  one they cannot choose. Only tab labels truncate.
- The card's padding lives on its rows and not on the card, so the rule under
  the tab strip runs edge to edge: a rule inside a padded box stops short of the
  corners it is squaring off, which reads as a line somebody drew rather than as
  the edge of a region.

### 5.8 Notice & error
A plain notice is one quiet muted line. An **error** notice — a real failure, not
a mild warning — takes the alert icon, `danger` and the body size, on a wash of
its own danger colour. Deliberately **not** one of the muted machine wells: those
say a machine produced this text, and here the tint is what carries the meaning.

*(Not rendered: the contract's end-of-turn "N files changed" summary block.)*

---

## 6 — Chrome & status rows

Transient rows that are not answers:

- **The session header**, above the transcript: what this conversation is
  called, what it is doing, and the things done *to* it — Find, and a `•••`
  holding Export, Resume in this session, Restart and Close; and a menu of the
  project's past conversations, each opening as a session of its own beside this
  one. Plus the two
  ways back to something the window has put away: the rail, while it is hidden,
  and the Workbench. Those are here because **the pane is mounted as a bare
  panel with no tab bar**, and this row is the only chrome it has.
  Drawn **quiet** — muted, no weight — because it names what is already on
  screen, and chrome that draws the eye is taking it from the conversation.
  A **hairline beneath it** separates the chrome from the answers: the two are
  read differently, and without the line the title reads as the first thing the
  conversation said.
  Separate from the composer's row because the two answer different questions.
  The composer's controls are about the message being written; these are about
  the conversation as a whole, and one row of seven buttons made every one of
  them equally easy to hit by accident.
  Resume is disabled mid-turn: it throws the running turn away exactly as a
  restart does, and a menu that has to be opened twice to be believed is a worse
  warning than an item that will not go.
- **Empty hint** before a session has been picked or a first prompt sent.
- **Turn status**, from the model rather than the view (`Chat::activity_status`),
  drawn in the header beside the title: "Waiting for your approval…" while a
  permission is parked, else a working line — and **nothing** while a live
  thought or a running tool is already saying it. The status exists to answer
  "is anything happening"; repeating what the block above already says is noise,
  not reassurance.
- **Jump to the latest** — a round `New activity` button resting on the composer's top edge,
  shown only while the list is scrolled away from its end *by the reader* — never
  while a newly asked question is being held at the top of the panel. It floats
  rather than taking a row of its own: a control that comes and goes cannot own
  layout, or the whole conversation shifts by its height each time the reader
  scrolls up and back down. A transcript too short to scroll shows nothing —
  "not at the bottom" and "there is no bottom to be away from" are different
  questions, and only the first of them has an answer worth a button.
  **Which of the two it is comes from the list's follow-tail state, not from its
  measured height.** A transcript follows its tail and stops following the
  moment the reader scrolls up; asked instead whether the scroll offset has
  reached the bottom, the list has to know the height of every run — and a list
  that measures rows lazily has never measured the ones above the viewport, so
  on any conversation long enough for the question to matter the answer came
  back *don't know* and the control stayed hidden. It appeared on short
  conversations and went missing on exactly the long ones it exists for.
  **Scrolling has to be asked to report itself.** The list owns its offset and
  moves it without telling the pane, so anything drawn *from* where the list is
  sitting needs a scroll handler behind it — without one this control waited for
  whatever happened to redraw the pane next, which on a finished conversation is
  nothing at all: the reader scrolled up and the way back only appeared once
  they touched something else.
- A session with no live conversation shows the **resume picker** instead of a
  transcript: choosing has to happen before anything connects, because connecting
  first would start a fresh conversation and archive it.

- **Connecting**, and the two halves of it are opposite. A conversation coming
  up for the **first time** shows nothing but the wait: its header, and a
  spinner naming what is being connected to. The archive is adopted the moment a
  conversation is picked, so drawing it would put a transcript, a composer and a
  ready-looking pane on screen seconds before a word could be sent to any of it
  — with nothing but a Send that refuses once pressed to say otherwise. A
  **re**connect is the reverse: on a restart, or an adapter respawned after it
  died, the conversation is already being read, and taking it away for the
  seconds a spawn costs reads as data loss — so the transcript stays and a strip
  over the composer carries the spinner instead. One rule stands between them,
  and it is about the conversation's own history rather than about who asked for
  the connect: *has this ever been live in this pane.*
  The header's status answers the link before it answers anything else — a turn
  cannot be in flight down a channel that is not up yet.
- **Opening** — before a session has either a picker or a transcript (the scan
  for past conversations, or a restart between adapters) the pane waits out
  loud. It used to show the hint for *no session here*, which told the user to
  start one they had already started.

*(Not rendered: the resumed divider between replayed history and new turns.)*

---

## 7 — Fold-state model

State lives **on the item, typed** — never in a global string-keyed set, never
keyed by render position.

| State | Where it lives |
|-------|----------------|
| tool card fold | `ToolItem.fold` |
| an `OUT` well un-folded | `ToolItem.out_open`, keyed by the section's index |
| plan fold | `PlanItem.fold` |
| thought | `Thought.expanded` |
| activity run | `ChatSession.activity_open`, keyed by the run's first item id |

The last row is the one exception, and it is bounded rather than free. A run is
not an item, so it has nothing to hang its fold on; the id that names it *is* a
render position, which is why the set holding it belongs to **one session** and
dies with it. Held one level up — on the pane, across every session — opening a
run in one conversation silently opened whatever sat at the same position in the
next one, and a restart, which folds the live tail back into history and
renumbers everything, moved every fold in place.

**Force-expand is computed, never stored** — there is no second set to keep in
sync:

```rust
impl ToolItem {   // only work changing right now force-opens
    pub fn is_open(&self) -> bool {
        self.fold || matches!(self.call.status, InProgress)
    }
}
```

`PlanItem` does the same while any entry is in progress.

**Code-block identity under streaming.** The answer's markdown re-parses as
tokens arrive, so "the Nth block of the parse" is not a stable identity. Any
per-block state a fold needs would have to be keyed by **fence-open order** — a
counter incremented when a fence *opens*, never renumbered on re-parse — so that
a fold made mid-stream survives the blocks arriving after it. The model keys
nothing by it today, because nothing folds a code block.

**Defaults:** tool cards and thoughts fold collapsed. Code blocks do not fold at
all in this build (§5.2).

---

## 8 — Bounded rendering

Every code, diff and output renderer draws **one element per line**, so unbounded
content would freeze the frame. The caps are named constants next to the
renderers: diff lines per card (across hunks), mono lines per well, the fold
threshold that hides the rest behind "Show N more lines", terminal lines, plan
items, attachment rows, code-block height, tool-detail height, completion rows —
plus `MAX_TERM_BYTES` in core, which bounds the model rather than the view.

An activity run needs no constant: its summary is built from a fixed array
indexed by category, so the number of phrases it can produce is bounded by the
number of categories that exist. A bound the type system already holds does not
get a second one written beside it.

Keep any new content rendering bounded, and **say on screen when a cap bit**.

---

## 9 — Streaming & in-flight states

The most-seen states are the ones where the turn is *arriving*.

- **Streaming prose** appends; the parse cache grows by the delta.
- **An unterminated code fence** renders as an open well, not as inline text
  waiting to become a block.
- **A pending tool** shows its header and a quiet "waiting for output" line
  rather than an empty bordered box.
- **A streaming terminal** appends live and pins to the last N lines; the exit
  footer is absent until the process exits.
- **Rule:** a block's finished and streaming appearances differ only by what is
  missing — never by layout, so nothing shifts when it completes.
- *(Not rendered: the streaming caret at the end of live prose.)*

---

## 10 — Error, interrupt & resolution

### 10.1 Failed tool
Status flips to `danger`, the body gains an `ERR` section above any output, and
the tool becomes a compact row like other settled work. Its failed status
remains visible in the header; the user opens the body when the output is
useful. No border or card wrapper is added solely because it failed.

### 10.2 Agent error
A model or transport failure mid-turn renders as the §5.8 error notice, not as a
tool failure — it belongs to the turn, not to a call.

### 10.3 Interrupted turn
Cancelling keeps everything already received: in-flight cards keep their last
output and stop. A stopped turn is a readable partial turn, not a discarded one.

### 10.4 Links
Links open through the app's handler, never by the renderer itself.

---

## 11 — Claude Code tool shapes

Most tools are §5.5. These carry enough structure to deserve their own body.

### 11.1 Plan / TodoWrite
A checklist card, not an output well. Over ACP the whole list is republished on
every change, so the current turn's card updates **in place** — keeping its fold
— and force-opens while any entry is in progress. Collapsed, the header carries
the count it hides (`N/M done`), since it is then the whole card. Each entry:
pending a muted dot (the icon set has no "not started" mark that is not just
noise, and the row still has to hold the marker column so the labels stay
aligned), in-progress a `warning` mark for elapsing time — a calendar, standing
in for the clock the bundled icon set does not carry — and completed a `success`
check with the label struck through and muted. Bounded.

### 11.2 MultiEdit
One `EDIT` section per hunk, each with its own path header, stacked in a single
card. The diff cap is **across all hunks**, not per hunk.

### 11.3 WebFetch / WebSearch
*(Not rendered as a result list.)* **Blocked on protocol data:** the adapter
delivers results as one opaque text blob, so these render as a standard `OUT`
well. Re-parsing prose back into titles and URLs by heuristic is not acceptable.

### 11.4 Task / sub-agent
*(Not rendered as a nested transcript.)* **Blocked on protocol data:** ACP streams
a sub-agent as a flat tool call; its inner turns never arrive as nested updates.

---

## 12 — Media & non-text content

- **An image in a *tool result*** renders as a bounded thumbnail. Decoded
  handles are cached by the payload's pointer identity, so a redraw never
  re-uploads megabytes.
- **An image attached to a prompt** is a bounded thumbnail with its name as the
  caption under it, addressed by path — gpui loads and caches a path-sourced
  image off the UI thread, so a row redrawn on every streamed chunk costs a
  lookup rather than a decode. The archive keeps paths and not bytes, so a file
  that has since moved leaves the row as its name. Nothing is drawn for an
  attachment the agent never received: a thumbnail there would claim it was seen.
- **An image *staged* in the composer** is a named chip and nothing more. The
  preview belongs to the transcript, where an attachment is a block of the
  conversation; in the tray it would be a strip of thumbnails taking room from
  the prompt the card exists to hold.
- **An unidentifiable payload** renders as a quiet placeholder row — never a raw
  byte dump. Guessing a format renders a broken image, and nothing is the more
  honest of the two.

---

## 13 — Where the values come from

Nothing in this document is a number, and that is the point.

- **Colour and radius:** `cx.theme()`, at the call site.
- **Size:** rems, so per-panel zoom reaches them (`crate::zoom`).
- **Caps:** named constants beside the renderer that needs them (§8), so the
  bound and the loop it bounds are read together.
- **The card shape:** one shared constructor (§4), not a shape re-derived per
  block — that is what keeps twelve block types looking like one transcript.
