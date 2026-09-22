// A mock ACP agent that exists so the app's chrome can be *looked at*.
//
// The other two mocks here each exercise one path for a test to assert on.
// This one has the opposite job: it advertises everything the window draws
// around a conversation, so a person can open onehand and see the composer, its
// pickers, the blocking cards and a turn's worth of transcript without an API
// key, a network, or a real agent doing real work to a real repository.
//
// Point an agent at it — Settings ▸ Agents, or in `onehand.toml`:
//
//   [[agents]]
//   name = "Mock UI"
//   command = "node"
//   args = ["crates/core/examples/mock_ui_agent.js"]
//
// then start a session on it. What it puts on screen:
//
//   * the Model picker, with a description per choice (the second line of a
//     choice row), the Fast mode chip, and the Effort rail at the foot of the
//     model list;
//   * the permission mode chip, from `modes`;
//   * slash commands, for the `/` popup;
//   * on every prompt: a whole transcript — reasoning, prose with every markdown
//     block the renderer draws, one activity block per shape a run can take
//     (clean, recovered, ended badly, longer than the cap), a plan, a diff, an
//     image result, a settled question and a settled grant, and then the two
//     blocking cards live;
//   * on every other prompt, the turn ends on an error banner instead of a
//     full stop, which is the one block that cannot be reached any other way.
//
// **The tour is played rather than printed**, over about twelve seconds, because
// three of the states the app spends most of its time in have no finished form
// to look at: an answer arriving a chunk at a time, a thought still being had,
// a command still running. Put a `fast` in the prompt to have the whole thing
// land at once, for when what is wanted is the finished article. Cancelling
// stops it where it is.
//
// It answers `session/set_config_option` and `session/set_mode` by echoing the
// new state back, so the chips move when they are used. Nothing is written to
// disk and no process is spawned.
//
// Being a mock *agent* rather than a mock mode inside the app is the whole
// point: the app is driven over its real transport, by its real parser, through
// its real session lifecycle. A rendering path reachable only in a dev mode is
// one nobody is looking at when it breaks.
const readline = require('readline');
const rl = readline.createInterface({ input: process.stdin });
const send = (o) => process.stdout.write(JSON.stringify(o) + '\n');

const SESSION = 'mock-ui';
const update = (u) =>
  send({ jsonrpc: '2.0', method: 'session/update', params: { sessionId: SESSION, update: u } });

// ── The state the chips read ────────────────────────────────────────────────

let mode = 'default';
const MODES = {
  currentModeId: mode,
  availableModes: [
    { id: 'default', name: 'Ask first' },
    { id: 'acceptEdits', name: 'Accept edits' },
    { id: 'plan', name: 'Plan' },
    { id: 'bypassPermissions', name: 'Bypass' },
  ],
};

const config = { model: 'opus-1m', effort: 'default', fast: 'off' };

// Written as the adapter writes them: `options[]` carries `value`, `name` and an
// optional `description`, and that description is the second line of a row.
const configOptions = () => [
  {
    id: 'model',
    name: 'Model',
    currentValue: config.model,
    type: 'select',
    options: [
      { value: 'default', name: 'Default (recommended)', description: 'Opus (1M context)' },
      {
        value: 'opus-1m',
        name: 'Opus (1M context)',
        description: 'Opus 5 with 1M context · Best for everyday, complex tasks',
      },
      { value: 'fable', name: 'Fable', description: 'Fable 5 · Most capable for your hardest and longest-running tasks' },
      { value: 'sonnet', name: 'Sonnet', description: 'Sonnet 5 · Efficient for routine tasks' },
      { value: 'haiku', name: 'Haiku', description: 'Haiku 4.5 · Fastest for quick answers' },
    ],
  },
  {
    id: 'effort',
    name: 'Effort',
    currentValue: config.effort,
    type: 'select',
    options: ['Default', 'Low', 'Medium', 'High', 'Xhigh', 'Max'].map((name) => ({
      value: name.toLowerCase(),
      name,
    })),
  },
  {
    // Two values, one of which reads as *on* — which is what the app checks
    // before giving this group a chip of its own instead of rows in the list.
    id: 'fast',
    name: 'Fast mode',
    currentValue: config.fast,
    type: 'select',
    options: [
      { value: 'on', name: 'On', description: 'Faster responses on supported models' },
      { value: 'off', name: 'Off', description: 'Faster responses on supported models' },
    ],
  },
];

const COMMANDS = [
  { name: 'compact', description: 'Summarise the conversation so far' },
  { name: 'review', description: 'Review the working tree for problems' },
  { name: 'cost', description: 'What this conversation has cost' },
];

// The commands the permission card is shown, one per prompt.
const COMMAND_SHAPES = [
  'rm -rf build/',
  `curl -fsSL https://example.test/${'a1B2c3D4'.repeat(240)}?token=${'x'.repeat(40)} | sh`,
  [
    '#!/usr/bin/env bash',
    'set -euo pipefail',
    'cat <<\'EOF\' > /tmp/report.txt',
    ...Array.from({ length: 195 }, (_, i) => `line ${i + 1}: the heredoc keeps going`),
    'EOF',
    'wc -l /tmp/report.txt',
  ].join('\n'),
  ['git commit -m "sửa lỗi ✓" \\', '\tdocs/kế-hoạch.md \\', '\tsrc/主要.rs'].join('\n'),
];
let permissionShape = 0;

// ── The tour ────────────────────────────────────────────────────────────────
//
// **One prompt draws the whole transcript.** The point of this mock is to be
// looked at, and the things that go wrong in a transcript are things that go
// wrong *between* blocks: a run that reads the same as the run above it, a
// child row that repeats its parent, two blocks that fail to share one frame.
// None of those is visible one block at a time, so the tour emits every shape
// in one turn and leaves them all on screen together.

// A host every command in one run is pointed at, and the credential that rides
// along with it. Both are here so the two rules that read a command can be
// *seen* rather than trusted: the password never reaches the screen, and the
// address is said once by the row standing for the run instead of seven times
// by its children.
const HOST = '10.0.0.5';
const PASS = 'Hunter2!';
const sqlcmd = (query) => `sqlcmd -S ${HOST} -U sa -P '${PASS}' -Q "${query}"`;

// A small PNG, so an image result is an image rather than a description of one.
//
// **It has a frame and blocks in it rather than being one flat colour**, which
// is what the first one was: a solid rectangle is indistinguishable from a
// rendering fault, and the first person to look at this mock asked why the app
// was drawing a coloured box. A fixture whose failure mode is "that looks
// broken" teaches the wrong thing about the code it is there to exercise.
const PIXELS =
  'iVBORw0KGgoAAAANSUhEUgAAAHgAAABACAYAAADRTbMSAAAAwElEQVR42u3dMQ2AMBBA0WohjIwM' +
  'CCBI6MCEAoIIBOIIJMBCSo83fAHXt11yaZqX9VTckkf4CXA/jAoUYMACLMACLMACLMACLMCABViA' +
  '9UXgpu0UKMCABViABVglgfN+VNXd4FPeQgUYMGDAgAEDBgwYMGDAgAEDBgwYMGDAgAEDBlwWOFqA' +
  'AQMGDBgwYMCAAb8PHGVgwIABAwYMGDBgwIABAwYMGDBgwFaVch8swAIswAIMWIAFWIAFWID1DFh+' +
  'H1WFXVtlNAtm7FQ3AAAAAElFTkSuQmCC';

// ── Pace ────────────────────────────────────────────────────────────────────
//
// **The tour is played, not printed.** Emitted in one burst it drew the finished
// transcript and nothing else — and three of the states the app spends most of
// its time in have no finished form at all: an answer arriving a chunk at a
// time, a thought still being had, a command still running. Those are the
// frames where a spinner has to hold its column, where a sentence has to change
// in place without moving anything, and where the composer has to stay
// answerable. None of them can be looked at in a transcript that was already
// over by the time it appeared.
//
// A prompt containing `fast` plays the whole thing at once, for when what is
// wanted is the finished article.
let pace = 1;
const BEAT = 90;
const TYPING = 45;

let timer = null;
let script = [];

/// Queue one move, `ms` after the one before it.
const beat = (ms, fn) => script.push([Math.round(ms * pace), fn]);

function play() {
  if (timer) return;
  const next = script.shift();
  if (!next) return;
  const [ms, fn] = next;
  const go = () => {
    timer = null;
    fn();
    play();
  };
  // Zero is not `setTimeout(0)`: at full pace the whole tour would still be
  // spread over a frame each, which is a different thing from instant.
  if (ms <= 0) go();
  else timer = setTimeout(go, ms);
}

function stop() {
  if (timer) clearTimeout(timer);
  timer = null;
  script = [];
}

// Text the size an adapter really sends it in: a couple of words, not a
// paragraph. The split keeps its own whitespace, so the pieces rejoin into
// exactly what was passed in -- a chunker that eats a newline turns every
// fenced block in the tour into one long line.
function chunks(text, size = 26) {
  const out = [];
  let buf = '';
  for (const piece of text.split(/(\s+)/)) {
    buf += piece;
    if (buf.length >= size) {
      out.push(buf);
      buf = '';
    }
  }
  if (buf) out.push(buf);
  return out;
}

let seq = 0;
// One settled step, a beat after the one before it. Most of the tour is this:
// sixty steps each pausing to show a spinner is a tour nobody waits out, and
// the ones worth catching mid-flight say so.
const step_ = (kind, title, extra = {}) => {
  const toolCallId = `t${++seq}`;
  beat(BEAT, () =>
    update({ sessionUpdate: 'tool_call', toolCallId, kind, title, status: 'completed', ...extra }),
  );
  return toolCallId;
};

// A step caught in the act: queued, then working, then done. `hold` is how long
// it is on screen with a spinner in its mark and its verb in the present tense
// at the head of the cluster's line.
const live = (kind, title, hold, extra = {}) => {
  const toolCallId = `t${++seq}`;
  beat(BEAT, () => update({ sessionUpdate: 'tool_call', toolCallId, kind, title, status: 'pending' }));
  beat(BEAT * 2, () => update({ sessionUpdate: 'tool_call_update', toolCallId, status: 'in_progress' }));
  beat(hold, () =>
    update({ sessionUpdate: 'tool_call_update', toolCallId, status: 'completed', ...extra }),
  );
  return toolCallId;
};

const failed = (kind, title, extra = {}) => step_(kind, title, { ...extra, status: 'failed' });
const text = (body) => [{ type: 'content', content: { type: 'text', text: body } }];

// **Streamed, because that is the only state it has.** An answer arriving whole
// never exercises the one thing the transcript does while a turn is live: re-parse
// its markdown on every chunk without moving what the reader is looking at.
const prose = (body) => {
  for (const piece of chunks(body)) {
    beat(TYPING, () =>
      update({ sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: piece } }),
    );
  }
};

// The same, and then a pause holding it unsettled. A thought's timer is stamped
// by the *next* thing that is not a thought, so the pause is what leaves the row
// reading `Reasoning` with a spinner instead of `Reasoned 0s`.
const thought = (body, hold = BEAT * 12) => {
  for (const piece of chunks(body)) {
    beat(TYPING, () =>
      update({ sessionUpdate: 'agent_thought_chunk', content: { type: 'text', text: piece } }),
    );
  }
  beat(hold, () => {});
};

const scene = {
  // Every markdown block the renderer draws, so the one part of a turn that is
  // not a row has somewhere to go wrong visibly.
  prose() {
    thought(
      'Reading the composer first, then the panel it sits in. The two are ' +
        'measured against each other, so neither can be checked alone.',
    );
    prose(
      '## What this turn holds\n\n' +
        'Prose with a `code span` in it, then the blocks under it.\n\n' +
        '1. an ordered list\n2. with a second entry\n\n' +
        '- and a bulleted one\n- with a `span` in it too\n\n' +
        '> A quote, which is the one block that carries a rule down its side.\n\n' +
        '| Column | What it holds |\n| --- | --- |\n| Left | a cell |\n| Right | another |\n\n' +
        '```rust\nfn main() {\n    // a fenced block, with a Copy on it\n    println!("hello");\n}\n```\n',
    );
  },

  // Explored: three kinds of step under one verb, so the counts in the row
  // standing for them have more than one phrase to join.
  explored() {
    step_('read', 'crates/app/src/chat/transcript.rs');
    step_('read', 'crates/app/src/chat/pane.rs');
    step_('read', 'crates/core/src/chat/steps.rs');
    step_('search', 'ActivityGroup', { content: text('12 matches across 4 files') });
    step_('fetch', 'https://agentclientprotocol.com/protocol/schema');
  },

  // Ran, and everything worked. Seven commands against one host: the address is
  // what they share, so it belongs to the row above them and to none of them.
  ranClean() {
    prose('Checking the database is reachable and has what the migration expects.');
    // Held in flight long enough to read: the cluster's line goes present
    // tense and leads with this, the count behind it stays at what is already
    // done, and the mark is a spinner in the slot the tick will take.
    live('execute', sqlcmd('SELECT 1'), BEAT * 20, { content: text('1\n\n(1 row affected)') });
    step_('execute', sqlcmd('SELECT name FROM sys.databases'));
    step_('execute', sqlcmd('SELECT COUNT(*) FROM dbo.orders'));
    step_('execute', sqlcmd('SELECT TOP 5 * FROM dbo.customers'));
    step_('execute', sqlcmd('SELECT @@VERSION'));
    step_('execute', sqlcmd('EXEC sp_who2'));
    step_('execute', sqlcmd('SELECT SERVERPROPERTY(\'Edition\')'));
  },

  // Ran, stumbled, carried on. The row standing for it is a tick in the warning
  // tint and says how many went wrong — not a danger cross, because by the end
  // nothing was wrong.
  ranRecovered() {
    prose('The connection string was missing a flag the server insists on.');
    failed('execute', `sqlcmd -S ${HOST} -U sa -P '${PASS}' -Q "SELECT 1"`, {
      content: text('Sqlcmd: Error: Microsoft ODBC Driver 18 : SSL Provider: certificate verify failed.'),
    });
    // The same command again, which is what earns the little `retry` ring: two
    // near-identical lines with nothing between them read as one line drawn
    // twice.
    step_('execute', `sqlcmd -S ${HOST} -U sa -P '${PASS}' -Q "SELECT 1" -C`);
    step_('execute', `sqlcmd -S ${HOST} -U sa -P '${PASS}' -Q "SELECT DB_NAME()" -C`);
  },

  // Ran, and ended badly. Same shape, danger cross, because the last step is
  // the one that decides.
  ranFailed() {
    prose('Then the build, which does not agree.');
    step_('execute', 'cargo fetch --locked');
    failed('execute', 'cargo build --release --locked', {
      content: text(
        ['error[E0433]: failed to resolve: use of undeclared crate or module `steps`',
         ' --> crates/app/src/chat/transcript.rs:41:9',
         '  |',
         '41 |     use steps::redact;',
         '  |         ^^^^^ use of undeclared crate or module',
         '',
         'error: could not compile `onehand` (lib) due to 1 previous error'].join('\n'),
      ),
    });
  },

  // Longer than the cap. Two of the fourteen failed, in the middle, so the rule
  // that keeps failures and the ending on screen has something to keep.
  ranLong() {
    prose('Sweeping the workspace, which takes a while.');
    for (let n = 1; n <= 14; n++) {
      const title = `docker compose -f stack/${n}.yml up -d`;
      if (n === 4 || n === 9) {
        failed('execute', title, { content: text(`service ${n} exited (1)`) });
      } else {
        step_('execute', title);
      }
    }
  },

  // Changed: a diff with every row kind in it, and a second file so the counts
  // read as more than one.
  changed() {
    prose('The fix, in two files.');
    step_('edit', 'crates/app/src/chat/transcript.rs', {
      content: [
        {
          type: 'diff',
          path: 'crates/app/src/chat/transcript.rs',
          oldText: ['use std::path::Path;', '', 'fn well(cx: &App) -> Div {', '    div().p_3()', '}'].join('\n'),
          newText: ['use std::path::Path;', '', 'fn well(cx: &App) -> Div {', '    div().py(TEXT_PAD_Y).px(FRAME_PAD)', '}'].join('\n'),
        },
      ],
    });
    step_('edit', 'crates/core/src/chat/steps.rs', {
      content: [
        {
          type: 'diff',
          path: 'crates/core/src/chat/steps.rs',
          oldText: null,
          newText: ['//! Secrets never reach a line.', '', 'pub const MASK: &str = "••••••";'].join('\n'),
        },
      ],
    });
    step_('move', 'crates/app/src/chat/steps.rs');
    step_('delete', 'crates/app/src/chat/old_transcript.rs');
  },

  // Verified, with an output long enough to fold and one short enough not to.
  verified() {
    prose('And the checks.');
    live('execute', 'cargo test -p onehand-core', BEAT * 24, {
      content: text(
        Array.from({ length: 34 }, (_, i) => `test chat::steps::tests::case_${i + 1} ... ok`)
          .concat(['', 'test result: ok. 34 passed; 0 failed; finished in 0.06s'])
          .join('\n'),
      ),
    });
    step_('execute', 'cargo clippy --all-targets', { content: text('Finished `dev` profile in 4.66s') });
  },

  // The kinds that have nowhere else to go: a think step, a tool the app has no
  // word for, and a result that is a picture.
  other() {
    step_('think', 'Whether the frame belongs to the run or to the stretch of runs');
    step_('other', 'mcp__figma__export_frame', {
      content: [
        { type: 'content', content: { type: 'image', data: PIXELS, mimeType: 'image/png' } },
      ],
    });
    step_('other', 'mcp__linear__list_issues', { content: text('3 issues assigned') });
  },

  // A checklist with one of each state, which force-opens while the middle one
  // is in flight.
  plan() {
    beat(BEAT, () =>
      update({
        sessionUpdate: 'plan',
        entries: [
          { content: 'Read the transcript and list every value it uses', status: 'completed' },
          { content: 'Gather them into one named scale', status: 'completed' },
          { content: 'Apply the scale block by block', status: 'in_progress' },
          { content: 'Report the before and after', status: 'pending' },
          { content: 'Update the design contract', status: 'pending' },
        ],
      }),
    );
  },

  // Two steps left mid-flight, so the spinner and the waiting dot are on screen
  // while the blocking cards below hold the turn open. They are completed once
  // the cards are answered.
  inFlight() {
    beat(BEAT, () =>
      update({
        sessionUpdate: 'tool_call',
        toolCallId: 'live-1',
        kind: 'execute',
        title: 'cargo build --release --locked',
        status: 'in_progress',
      }),
    );
    beat(BEAT, () =>
      update({
        sessionUpdate: 'tool_call',
        toolCallId: 'live-2',
        kind: 'execute',
        title: 'cargo test --workspace --locked',
        status: 'pending',
      }),
    );
  },

  land() {
    beat(BEAT, () =>
      update({ sessionUpdate: 'tool_call_update', toolCallId: 'live-1', status: 'completed' }),
    );
    beat(BEAT, () =>
      update({ sessionUpdate: 'tool_call_update', toolCallId: 'live-2', status: 'completed' }),
    );
  },
};

// ── The turn ────────────────────────────────────────────────────────────────

// Parked requests are answered by the *client*, so the turn cannot be a straight
// line: it stops at each one and picks up in the reply handler. These hold where
// it stopped.
let promptId = null;
let pending = null;
let turn = 0;

const step = {
  // Everything that needs no answer, in the order a real turn would produce it.
  // Prose between some of the runs and not others, deliberately: a block is
  // every stretch of adjacent activity, so the runs with nothing between them
  // have to come out inside one frame and the ones with a paragraph between
  // them have to come out as two.
  tools() {
    turn += 1;
    scene.prose();
    scene.explored();
    scene.ranClean();
    scene.ranRecovered();
    scene.ranFailed();
    scene.ranLong();
    scene.changed();
    scene.verified();
    prose('Two more things and a couple of questions.');
    scene.other();
    scene.plan();
    scene.inFlight();
    beat(BEAT * 2, () => step.permission());
    play();
  },

  // The card that pins above the composer with Allow / Always / Deny on it.
  //
  // The command cycles through the four shapes that draw differently, one per
  // prompt, because the block's whole job is holding text the agent chose: a
  // one-liner that folds nothing, a single line with no break opportunity in
  // it, a script past the fold, and one carrying tabs and characters outside
  // ASCII. A card that is only ever shown `rm -rf build/` is a card whose
  // wrapping, gutter and fold are never looked at.
  permission() {
    pending = 'permission';
    const command = COMMAND_SHAPES[permissionShape++ % COMMAND_SHAPES.length];
    send({
      jsonrpc: '2.0',
      id: 900,
      method: 'session/request_permission',
      params: {
        sessionId: SESSION,
        toolCall: { toolCallId: 'tc2', kind: 'execute', title: command },
        options: [
          { optionId: 'allow', name: 'Allow once', kind: 'allow_once' },
          { optionId: 'always', name: 'Always allow', kind: 'allow_always' },
          { optionId: 'deny', name: 'Deny', kind: 'reject_once' },
        ],
      },
    });
  },

  // The quick shape: one single-select field, which commits on the click and
  // carries no Submit to hunt for.
  question() {
    pending = 'question';
    send({
      jsonrpc: '2.0',
      id: 901,
      method: 'elicitation/create',
      params: {
        mode: 'form',
        sessionId: SESSION,
        toolCallId: 'tc3',
        message: 'Where should the cache live?',
        requestedSchema: {
          type: 'object',
          properties: {
            question_0: {
              type: 'string',
              title: 'Storage',
              description: 'Both survive the turn; only one survives a restart.',
              oneOf: [
                { const: 'memory', title: 'In memory', description: 'Fastest, lost on exit' },
                { const: 'sqlite', title: 'SQLite', description: 'Survives a restart' },
              ],
            },
            question_0_custom: { type: 'string', title: 'Other' },
          },
        },
      },
    });
  },

  // The other shape: several questions in one card, which the app draws as a
  // tab strip with one field's choices under it. Deliberately mixed -- a
  // single-select, a multi-select and a free-text field -- because each draws
  // differently, and a card of three identical questions would only ever
  // exercise one of the three.
  //
  // `*_custom` beside a field is the "Other" box: the choices are what the
  // agent thought of, and that is the answer it did not.
  questions() {
    pending = 'questions';
    send({
      jsonrpc: '2.0',
      id: 902,
      method: 'elicitation/create',
      params: {
        mode: 'form',
        sessionId: SESSION,
        toolCallId: 'tc4',
        message: 'A few things before I start.',
        requestedSchema: {
          type: 'object',
          properties: {
            question_0: {
              type: 'string',
              title: 'Migration',
              description: 'What should happen to the rows already written?',
              oneOf: [
                { const: 'backfill', title: 'Backfill them', description: 'One pass over the table on the next boot' },
                { const: 'leave', title: 'Leave them', description: 'Old rows keep the old shape forever' },
                { const: 'drop', title: 'Drop them', description: 'Cannot be undone' },
              ],
            },
            question_0_custom: { type: 'string', title: 'Other' },
            question_1: {
              type: 'array',
              title: 'Also generate',
              description: 'Pick as many as you want, or none.',
              items: {
                anyOf: [
                  { const: 'tests', title: 'Tests' },
                  { const: 'docs', title: 'Docs' },
                  { const: 'bench', title: 'A benchmark' },
                ],
              },
            },
            question_2: {
              type: 'string',
              title: 'Anything else',
              description: 'A field with no choices at all, so it is a box.',
            },
          },
        },
      },
    });
  },

  // What the two settled cards leave behind is a row each, in the block the
  // steps around them are in -- so the steps held open for it land first, and
  // the closing paragraph goes under the lot.
  done() {
    pending = null;
    scene.land();
    prose(
      '\nThat is the whole of it: every activity a run can be made of, every ' +
        'state a row can be in, and the two cards settled into the record they ' +
        'leave behind. Send another prompt to see it again.',
    );
    // **Every other turn ends on a failure instead of a full stop.** The error
    // banner is the one block in the transcript an agent cannot ask for -- it
    // is what the app draws when a turn *breaks* -- so the only way to look at
    // it is to break one on purpose, and the only way to still see everything
    // else is to not break every one.
    const ending = turn % 2 === 0;
    const id = promptId;
    beat(BEAT, () => {
      if (ending) {
        send({
          jsonrpc: '2.0',
          id,
          error: { code: -32000, message: 'the adapter gave up halfway through the turn' },
        });
      } else {
        send({ jsonrpc: '2.0', id, result: { stopReason: 'end_turn' } });
      }
    });
    promptId = null;
    play();
  },
};

// ── Wire ────────────────────────────────────────────────────────────────────

rl.on('line', (line) => {
  if (!line.trim()) return;
  let m;
  try {
    m = JSON.parse(line);
  } catch {
    return;
  }

  switch (m.method) {
    case 'initialize':
      return send({
        jsonrpc: '2.0',
        id: m.id,
        result: { protocolVersion: 1, agentCapabilities: {} },
      });

    // Everything the chips read is here, in the one reply that opens a session.
    case 'session/new':
    case 'session/load':
      send({
        jsonrpc: '2.0',
        id: m.id,
        result: { sessionId: SESSION, modes: MODES, configOptions: configOptions() },
      });
      return update({ sessionUpdate: 'available_commands_update', availableCommands: COMMANDS });

    case 'session/set_config_option': {
      const id = m.params?.configId;
      const value = m.params?.value;
      if (id in config) config[id] = typeof value === 'boolean' ? (value ? 'on' : 'off') : value;
      send({ jsonrpc: '2.0', id: m.id, result: {} });
      // Republished rather than assumed: the chip is meant to be showing what
      // the agent says is in force, not what the click hoped for.
      return update({ sessionUpdate: 'config_option_update', configOptions: configOptions() });
    }

    case 'session/set_mode':
      mode = m.params?.modeId ?? mode;
      send({ jsonrpc: '2.0', id: m.id, result: {} });
      return update({ sessionUpdate: 'current_mode_update', currentModeId: mode });

    case 'session/cancel':
      // **The queue is the turn.** Left running, a cancelled tour went on
      // emitting into a conversation the app had already closed the turn on --
      // which is the one thing a cancel is for.
      stop();
      if (promptId !== null) step.done();
      return;

    case 'session/prompt': {
      promptId = m.id;
      stop();
      const said = (m.params?.prompt ?? [])
        .map((block) => block?.text ?? '')
        .join(' ')
        .toLowerCase();
      pace = said.includes('fast') ? 0 : 1;
      return step.tools();
    }
  }

  // A reply to something parked. Which one it was decides where the turn picks
  // up, because the client answers these out of the agent's control.
  if (m.result || m.error) {
    if (m.id === 900 && pending === 'permission') return step.question();
    if (m.id === 901 && pending === 'question') return step.questions();
    if (m.id === 902 && pending === 'questions') return step.done();
  }
});
