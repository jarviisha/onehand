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
//   * on every prompt: reasoning, prose, a tool call that completes, a parked
//     permission, a one-question card and a three-question card — every shape
//     the two blocking cards take.
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

// ── The turn ────────────────────────────────────────────────────────────────

// Parked requests are answered by the *client*, so the turn cannot be a straight
// line: it stops at each one and picks up in the reply handler. These hold where
// it stopped.
let promptId = null;
let pending = null;

const step = {
  // A tool call that arrives running and then completes, which is the pair of
  // updates a card is drawn from.
  tools() {
    update({ sessionUpdate: 'agent_thought_chunk', content: { type: 'text', text: 'Looking at the composer first.' } });
    update({
      sessionUpdate: 'agent_message_chunk',
      content: {
        type: 'text',
        text: 'Here is a turn with everything in it.\n\n- prose, with a `code span`\n- a list\n\n```rust\nfn main() {\n    println!("and a fenced block");\n}\n```\n',
      },
    });
    update({
      sessionUpdate: 'tool_call',
      toolCallId: 'tc1',
      title: 'crates/app/src/chat/composer.rs',
      kind: 'read',
      status: 'in_progress',
    });
    update({ sessionUpdate: 'tool_call_update', toolCallId: 'tc1', status: 'completed' });
    step.permission();
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

  done() {
    pending = null;
    update({
      sessionUpdate: 'agent_message_chunk',
      content: { type: 'text', text: '\nThat is the whole of the chrome. Send another prompt to see it again.' },
    });
    send({ jsonrpc: '2.0', id: promptId, result: { stopReason: 'end_turn' } });
    promptId = null;
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
      if (promptId !== null) step.done();
      return;

    case 'session/prompt':
      promptId = m.id;
      return step.tools();
  }

  // A reply to something parked. Which one it was decides where the turn picks
  // up, because the client answers these out of the agent's control.
  if (m.result || m.error) {
    if (m.id === 900 && pending === 'permission') return step.question();
    if (m.id === 901 && pending === 'question') return step.questions();
    if (m.id === 902 && pending === 'questions') return step.done();
  }
});
