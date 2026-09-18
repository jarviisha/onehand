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

The transcript runs in a centred **64rem reading column**. That is a maximum
rather than a fixed minimum: on a narrower panel the column contracts to the
available width, minus equal padding on both sides. **It is set by what the
widest block holds, not by prose alone.** A column sized purely for reading
sentences is the narrower number this used to be, and what that cost was
everything in the transcript which is not a sentence: a unified diff wrapping
its longest lines, a command well folding a path that would have fit, a tool
card's header ellipsizing a file name whose tail is the part that identifies
it. Those are the blocks somebody is reading the transcript *for* when
something has gone wrong, and a cap tuned past them to keep paragraphs
comfortable trades the case that matters for the case that was already fine.
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
  *ends* with. An opened activity group ends as a block's worth of reading, so
  what follows it takes a block gap — but it still begins with the same index row
  it began with while closed, and nothing about the boundary above that row
  changed. Read from the run as a whole, opening a group tripled the space over
  its own header: the row slid down under the pointer that had just clicked it,
  and everything above appeared to shift for a reason nothing on screen gave.
- **What a group opens into keeps the cadence index rows keep everywhere else.**
  Its members are the same quiet rows that sit at the smallest gap out in the
  transcript, so they sit at that gap inside it too. One list drawn at two
  rhythms depending on whether it is inside a group is the group deciding
  something that is not its to decide.
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
the first. A tool card, a plan, an activity strip and every descriptor on them
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
| Every quiet disclosure row — activity strips, thoughts, settled tool rows and their descriptors | `text_xs` |
| Attention tool names and "Plan" | semibold, at their card's size |
| A quiet row's *name* — an activity group, "Thought for Xs" | semibold, at `text_xs` |
| A quiet row's *summary* or descriptor | regular + `muted_foreground`, at `text_xs` |
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

One interaction with two shapes, selected by state rather than by adjacency.
Work needing attention is a card; settled history is a quiet disclosure row.

```
┌─ «icon» title · descriptor …… status  ▸ ┐   attention card
│  body (revealed on expand)               │
└──────────────────────────────────────────┘

«icon» settled summary  ✓ / failed  ▸        quiet disclosure
        body (revealed below)
```

- **Attention card:** pending and running tools, plus plans. Transparent surface,
  one hairline, themed radius and `p_3`; title-to-body spacing is `gap_2`. A
  plan's entries stay denser at `gap_1` inside their own list.
- **Quiet disclosure:** thoughts, activity strips and terminal tool rows. No
  border or card padding around the header or the whole group; failed tools use
  the same shape as completed tools and add only their `danger` status. Detail
  appears beneath the row. Activity summaries and settled tool rows start on
  the same axis as prose; Thought remains aligned there as a named block too.
  **Every row of this shape is one size**, thoughts included. A `Reasoned` strip
  holds thoughts *and* `Think` tool steps, so a thought set one step larger put
  two sizes among sibling rows and made the children louder than the group
  header naming them. Weight separates a name from a summary; size does not have
  to.
- **Header:** role icon, label or title + descriptor, optional status, then a
  chevron when detail exists. All glyphs occupy the same centred `size_4` slot;
  only the sans label and mono descriptor align by text baseline. Right means
  collapsed; down means open. The chevron always follows the label's right edge,
  never a distant right-hand rail; hovering the interactive row promotes the
  label from `muted_foreground` to `foreground`.
- **Body:** rendered only while expanded. Everything below one activity header
  — all `IN` / `OUT` / `EDIT` / `ERR` sections together — shares one bordered
  card whose left edge aligns with the label. Long detail scrolls inside that
  card, with a visible scrollbar whenever content overflows; the muted machine
  wells inside do not repeat its border. Running tools force-open; every settled
  state follows the user's fold choice.
- **Motion:** none. The chevron swaps rather than rotates; body height is not
  animated. A transcript that reflows while it streams is harder to read, not
  livelier.

---

## 5 — Block types

The `ChatItem` variants — `User · Agent · Thought · Tool · Plan · Permission ·
Ask · Notice` — plus the activity strip and the chrome rows (§6).

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
Collapsed reasoning, never containing tool calls. A §4 quiet disclosure whose summary is
"Thought for Xs" / "Thinking…", body on a well, collapsed by default.

### 5.4 Activity strip
Folds a run of adjacent **settled** process steps into one line. Pending and
running work never enters a strip: it remains visible as a card until it reaches
a terminal completed or failed state.

- Summary aggregates adjacent settled work by semantic kind (`Inspected 3 files
  · ran 1 command`) rather than repeating one phrase per target. If any member
  failed, the summary carries one `danger` failed state. Every strip starts with
  its stable group name and a distinct group icon; the changing counts remain a
  descriptor, so two different kinds of work never look like the same unnamed
  history row.
- Expanded steps are **indented one icon column beneath the summary** so the
  group hierarchy remains visible — without adding a card or left rule around
  the group. Each step's complete machine detail owns one bordered card aligned
  beneath that step's label. A group has no fixed-height viewport of its own;
  only an opened leaf detail is capped and scrollable.
- Settled strips follow the user's fold state. Live thoughts remain standalone
  rows and running tools remain attention cards, so the turn-level `busy` flag
  never expands an audit trail behind them.
- Fold state is keyed by the run's **first item identity**, never by run index: a
  new step joining the run ahead of it renumbers every index below and silently
  moves the fold.

### 5.5 Tool call
Header anatomy: kind icon + tool name + descriptor (first line, truncated; mono
for a command, sans otherwise) + status + disclosure when detail exists. ACP
titles that repeat the structured kind are normalized, so a row never reads
`Edit Edit …`. An execute step with an agent-authored human description omits
the redundant `Run`; raw commands retain it because it names the machine text.
Embedded newlines in a raw command are collapsed to spaces in the header, while
the expandable `IN` body preserves the command verbatim. Sans tool labels and
mono commands align by text baseline rather than by the centres of their
different font boxes.
File targets under the session root display as project-relative paths; opening
them still resolves the original path against that root.

- **Prominent vs quiet is decided only by status.** Terminal tools — completed
  or failed — render as chevroned ghost rows, whether alone or beside another
  tool. Pending and running tools render as full §4 cards and stay outside
  activity strips. Opening a terminal row sets its detail beneath it, by space
  and never by a rule (§5.4). Adjacency may replace several settled rows with
  one summary, but it never changes any tool's underlying shape.
- **Status:** pending muted · running `warning` · failed `danger`. Pending and
  running remain words because they need attention; failed remains a word
  because the exceptional terminal result must be named. Completed is the
  common case and becomes a compact `success` check beside the descriptor,
  avoiding a right-hand column of repeated `done`s.
- **The chevron follows the label, not the row.** It is the handle for the thing
  named beside it, including one-phrase activity summaries and thoughts. The
  label may shrink and truncate, but never flexes merely to send its chevron to
  the far edge. An exceptional status such as `failed` sits inside this cluster
  between descriptor and chevron, never in a separate right-hand column. Hover
  promotes the label text to `foreground` while semantic status colours remain
  unchanged.
- **Expanded when** the user opened it **or** it is running — computed, never
  stored (§7). Failed tools start collapsed like other settled work; their
  `danger` status remains visible in the header.
- **Body sections** are a tag column (`IN` / `OUT` / `EDIT` / `ERR`) beside a mono
  body:
  - `IN` — the command, verbatim. An execute step always has one, so it always
    offers disclosure: the header elides to a line, and a command is the part
    of a tool step most worth reading whole.
  - `EDIT` — the diff: a path sub-header, then removals and additions on tinted
    lines, capped **across all hunks of the card**.
  - `OUT` (text) — the tail, folded to a reading threshold with "Show N more
    lines", then hard-capped per well when opened. Two bounds answering two
    questions: the fold is about not burying the answer below it, the cap is
    about not drawing one element per line of whatever the agent `cat`-ed. The
    fold state is the model's, keyed by the section's index.
  - `OUT` (terminal) — the live ACP-terminal stream, last N lines, then an exit
    footer that reads `success` on 0 and `danger` otherwise.
- A tool with no detail renders as its header line only.
- **The path in a diff header is a link** — clicking it asks the shell to open
  the file in the Workbench. The chat never opens a file itself: it says what was
  asked for, and the shell decides where it goes.

### 5.6 Command
The terminal icon and mono descriptor distinguish a command — **no accent
rail** (§3). While pending or running it is a standalone card; once completed
or failed it becomes the same compact row as every other settled tool and may
join adjacent settled work in an activity strip.

### 5.7 Permission
A blocking card: the agent parks until it is answered.

- A `warning` icon and a short heading; the agent-authored command sits in a
  bounded mono well.
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

An activity strip needs no constant: its summary is built from a fixed array
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
