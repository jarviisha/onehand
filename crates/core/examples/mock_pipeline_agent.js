// A mock ACP agent that plays its part in a pipeline run, so every path the
// driver can take is walked in seconds, without an API key, against a real
// checkout.
//
//   [[agents]]
//   name = "Mock pipeline"
//   command = "node"
//   args = ["/absolute/path/to/onehand-gpui/crates/core/examples/mock_pipeline_agent.js"]
//
// **It reads what a step wants from the rules onehand appends to the prompt**,
// never from the template's own wording, so an edited template still drives
// it: "Do not edit any file" is a plan, "has to change the code" is a change,
// "Commit your work" asks for a commit. A carry-on prompt is answered the
// same way, from what onehand says the last turn missed.
//
// Its orders come from the brief, as `fast` does for the UI mock:
//
//   * `miss` — every turn does nothing and says nothing, so the step's gates
//     miss until the run is exhausted;
//   * `fail-check` — the first change writes `check: fail` into
//     `mock-pipeline.txt`; the change after a failed check writes
//     `check: pass`. Pair it with a check command such as
//     `sleep 5 && grep -q 'check: pass' mock-pipeline.txt`;
//   * `fast` — answer at once instead of over about six seconds, which is
//     what leaves room to press Stop or quit mid-step.
//
// Cancelling ends the turn where it is, with `cancelled`.
const readline = require('readline');
const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

const rl = readline.createInterface({ input: process.stdin });
const send = (o) => process.stdout.write(JSON.stringify(o) + '\n');
const say = (session, text) =>
  send({
    jsonrpc: '2.0',
    method: 'session/update',
    params: {
      sessionId: session.id,
      update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text } },
    },
  });

// **Every session has an id and a state of its own**: one id shared by every
// session would file their transcripts under one name.
//
// A session's state: where it works, its turn under way, and the brief's
// orders, kept for the carry-on prompts that do not repeat them. A step's
// prompt carries the brief above onehand's `---`; only that part is read.
const sessions = new Map(); // id → { id, cwd, turn: { id, timers } | null, orders }

// What the prompt asks of this turn, and what the turn does about it.
function play(session, prompt) {
  if (prompt.includes('\n---\n')) session.orders = prompt.split('\n---\n')[0].toLowerCase();
  const { orders, cwd } = session;
  if (orders.includes('miss')) return { lines: [], act: () => {} };

  const change =
    prompt.includes('The turn has to change the code.') || prompt.includes('that turn changed nothing');
  const commit =
    prompt.includes('Commit your work on this branch') ||
    prompt.includes('commit it on this branch') ||
    prompt.includes('no commit has') ||
    prompt.includes('no new commit');
  const failed = prompt.includes('The check failed:');

  if (!change && !commit) {
    return {
      lines: [
        'The plan: ',
        'add `mock-pipeline.txt` at the top of the checkout, ',
        'with one line the check reads. ',
        'The check passing is how this is known to work.',
      ],
      act: () => {},
    };
  }
  const verdict = orders.includes('fail-check') && !failed ? 'fail' : 'pass';
  return {
    lines: ['Writing `mock-pipeline.txt`. ', commit && 'Committing it. ', `Done (check: ${verdict}).`].filter(Boolean),
    act: () => {
      if (change || failed) {
        fs.writeFileSync(path.join(cwd, 'mock-pipeline.txt'), `check: ${verdict}\n${Date.now()}\n`);
      }
      if (commit) {
        const git = (...args) => execFileSync('git', args, { cwd, stdio: 'pipe' });
        git('add', '-A');
        git('commit', '-q', '-m', 'mock pipeline change');
      }
    },
  };
}

function end(session, stopReason) {
  if (!session?.turn) return;
  session.turn.timers.forEach(clearTimeout);
  send({ jsonrpc: '2.0', id: session.turn.id, result: { stopReason } });
  session.turn = null;
}

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
      return send({ jsonrpc: '2.0', id: m.id, result: { protocolVersion: 1, agentCapabilities: {} } });
    case 'session/new': {
      const id = `mock-pipeline-${process.pid}-${Date.now()}-${sessions.size}`;
      sessions.set(id, { id, cwd: m.params?.cwd ?? process.cwd(), turn: null, orders: '' });
      return send({ jsonrpc: '2.0', id: m.id, result: { sessionId: id } });
    }
    case 'session/cancel':
      return end(sessions.get(m.params?.sessionId), 'cancelled');
    case 'session/prompt': {
      const session = sessions.get(m.params?.sessionId);
      if (!session) {
        return send({ jsonrpc: '2.0', id: m.id, error: { code: -32602, message: 'unknown session' } });
      }
      end(session, 'cancelled');
      const prompt = (m.params?.prompt ?? []).map((b) => b?.text ?? '').join('\n');
      const { lines, act } = play(session, prompt);
      const pace = session.orders.includes('fast') ? 0 : 6000 / (lines.length + 1);
      const turn = (session.turn = { id: m.id, timers: [] });
      lines.forEach((text, i) => turn.timers.push(setTimeout(() => say(session, text), pace * i)));
      turn.timers.push(
        setTimeout(() => {
          try {
            act();
          } catch (e) {
            say(session, `\n\nThe mock could not touch the work: ${e.message}`);
          }
          end(session, 'end_turn');
        }, pace * (lines.length + 1)),
      );
      return;
    }
  }
});
