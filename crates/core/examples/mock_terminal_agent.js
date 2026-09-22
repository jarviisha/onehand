// A minimal mock ACP agent that exercises the terminal extension: on a prompt
// it asks the client to create a terminal, references it in a tool call, waits
// for it to exit, then ends the turn. Used to verify src/acp/terminal.rs.
//
// **It exits non-zero, and that is the point of the number.** An exit status is
// the one fact the protocol carries nowhere outside this extension, so a
// transcript row can only ever print `exit N` for a command that ran through
// here -- and a mock that always succeeded left the whole of that path
// unreachable by anything but a real agent breaking a real build. The counter
// runs to five and then fails, so the row shows output *and* a status.
const readline = require('readline');
const rl = readline.createInterface({ input: process.stdin });
let promptId = null;
const send = (o) => process.stdout.write(JSON.stringify(o) + '\n');

rl.on('line', (line) => {
  if (!line.trim()) return;
  let m;
  try { m = JSON.parse(line); } catch { return; }

  if (m.method === 'initialize') {
    send({ jsonrpc: '2.0', id: m.id, result: { protocolVersion: 1, agentCapabilities: {} } });
  } else if (m.method === 'session/new') {
    send({ jsonrpc: '2.0', id: m.id, result: { sessionId: 'mock' } });
  } else if (m.method === 'session/prompt') {
    promptId = m.id;
    send({
      jsonrpc: '2.0', id: 100, method: 'terminal/create',
      params: { sessionId: 'mock', command: 'sh',
        args: ['-c', 'for i in 1 2 3 4 5; do echo "line $i"; sleep 0.3; done; echo "error: it did not work" >&2; exit 101'] },
    });
  } else if (m.id === 100 && m.result) {
    const tid = m.result.terminalId;
    send({ jsonrpc: '2.0', method: 'session/update', params: { sessionId: 'mock',
      update: { sessionUpdate: 'tool_call', toolCallId: 'tc1', title: 'Run counter',
        kind: 'execute', status: 'in_progress', content: [{ type: 'terminal', terminalId: tid }] } } });
    send({ jsonrpc: '2.0', id: 101, method: 'terminal/wait_for_exit',
      params: { sessionId: 'mock', terminalId: tid } });
  } else if (m.id === 101 && m.result) {
    send({ jsonrpc: '2.0', method: 'session/update', params: { sessionId: 'mock',
      update: { sessionUpdate: 'tool_call_update', toolCallId: 'tc1',
        // The reply is `{ exitStatus: { exitCode } }` -- read through it, or
        // every run reports success whatever the child did.
        status: m.result.exitStatus?.exitCode ? 'failed' : 'completed' } } });
    send({ jsonrpc: '2.0', id: promptId, result: { stopReason: 'end_turn' } });
  }
});
