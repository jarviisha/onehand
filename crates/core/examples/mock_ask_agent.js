// A minimal mock ACP agent that exercises the *elicitation* path — the wire
// shape Claude's adapter uses for its `AskUserQuestion` tool. On a prompt it
// sends one `elicitation/create` (a single-select question plus its "Other"
// box, then a multi-select), prints the client's answer to stderr, and ends the
// turn. Used to verify the parse/park/answer flow in src/acp/client.rs.
//
//   ACP_CMD="node examples/mock_ask_agent.js" cargo run --example acp_smoke go
//
// It also asserts the client advertised `elicitation.form`: without that
// capability the real adapter disables AskUserQuestion outright, so losing it
// is exactly the regression this mock exists to catch.
const readline = require('readline');
const rl = readline.createInterface({ input: process.stdin });
let promptId = null;
const send = (o) => process.stdout.write(JSON.stringify(o) + '\n');

rl.on('line', (line) => {
  if (!line.trim()) return;
  let m;
  try { m = JSON.parse(line); } catch { return; }

  if (m.method === 'initialize') {
    const form = m.params?.clientCapabilities?.elicitation?.form;
    process.stderr.write(`[mock] client elicitation.form: ${JSON.stringify(form)}\n`);
    if (!form) {
      process.stderr.write('[mock] FAIL: no form-elicitation capability — the real ' +
        'adapter would disable AskUserQuestion here\n');
      process.exit(1);
    }
    send({ jsonrpc: '2.0', id: m.id, result: { protocolVersion: 1, agentCapabilities: {} } });
  } else if (m.method === 'session/new') {
    send({ jsonrpc: '2.0', id: m.id, result: { sessionId: 'mock' } });
  } else if (m.method === 'session/prompt') {
    promptId = m.id;
    send({
      jsonrpc: '2.0', id: 200, method: 'elicitation/create',
      params: {
        mode: 'form',
        sessionId: 'mock',
        toolCallId: 'tc1',
        message: 'Please answer the following questions.',
        requestedSchema: {
          type: 'object',
          properties: {
            question_0: {
              type: 'string',
              title: 'Storage',
              description: 'Where should the cache live?',
              oneOf: [
                { const: 'memory', title: 'In memory', description: 'Fastest, lost on exit' },
                { const: 'sqlite', title: 'SQLite', description: 'Survives a restart' },
              ],
            },
            question_0_custom: { type: 'string', title: 'Other' },
            question_1: {
              type: 'array',
              title: 'Also generate',
              items: { anyOf: [{ const: 'tests', title: 'Tests' }, { const: 'docs', title: 'Docs' }] },
            },
            question_1_custom: { type: 'string', title: 'Other' },
          },
        },
      },
    });
  } else if (m.id === 200 && (m.result || m.error)) {
    process.stderr.write(`[mock] answer: ${JSON.stringify(m.result ?? m.error)}\n`);
    send({ jsonrpc: '2.0', method: 'session/update', params: { sessionId: 'mock',
      update: { sessionUpdate: 'agent_message_chunk',
        content: { type: 'text', text: `Got: ${JSON.stringify(m.result?.content ?? {})}` } } } });
    send({ jsonrpc: '2.0', id: promptId, result: { stopReason: 'end_turn' } });
  }
});
