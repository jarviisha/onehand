// A workflow-shaped mock ACP agent, for driving runs without an API key.
//
// Every prompt streams a short answer over about four seconds, so a Stop can
// land mid-turn. A prompt asking for "the change" (an implement step) first
// appends a line to README.md in its working directory, so the step's gates
// see the code changed, and commits it when the prompt asks for a commit (a
// branch run); any other prompt (a plan) changes nothing.
// `session/cancel` ends the turn as cancelled. No cards, no errors: unlike
// the Mock UI agent, a workflow run on it goes from its first step to its last.
const fs = require('fs');
const { execFileSync } = require('child_process');
// Its own, as a real agent's is: onehand keeps a transcript by session id.
const session = `mw-${process.pid}-${Date.now()}`;
const rl = require('readline').createInterface({ input: process.stdin });
const send = (o) => process.stdout.write(JSON.stringify(o) + '\n');
const up = (u) => send({ jsonrpc: '2.0', method: 'session/update', params: { sessionId: session, update: u } });
let turn = null, timers = [];
rl.on('line', (line) => {
  let m; try { m = JSON.parse(line); } catch { return; }
  if (m.method === 'initialize') send({ jsonrpc: '2.0', id: m.id, result: { protocolVersion: 1, agentCapabilities: {} } });
  else if (m.method === 'session/new') send({ jsonrpc: '2.0', id: m.id, result: { sessionId: session } });
  else if (m.method === 'session/prompt') {
    turn = m.id;
    const text = (m.params.prompt || []).map((b) => b.text || '').join('');
    const change = text.includes('This step is the change');
    if (change) fs.appendFileSync('README.md', `changed at ${Date.now()}\n`);
    if (change && /commit your work/i.test(text)) {
      execFileSync('git', ['-c', 'user.name=mock', '-c', 'user.email=mock@localhost', 'commit', '-qam', 'Mock change']);
    }
    const words = (change ? 'I made the change to README.md.' : 'Plan: append one line to README.md, then run the check.').split(' ');
    words.forEach((w, i) => timers.push(setTimeout(() => up({ sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: w + ' ' } }), 400 * (i + 1))));
    timers.push(setTimeout(() => { send({ jsonrpc: '2.0', id: turn, result: { stopReason: 'end_turn' } }); turn = null; }, 400 * (words.length + 1)));
  } else if (m.method === 'session/cancel' && turn !== null) {
    timers.forEach(clearTimeout); timers = [];
    send({ jsonrpc: '2.0', id: turn, result: { stopReason: 'cancelled' } }); turn = null;
  }
});
