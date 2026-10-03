// A mock ACP agent that plays its part in a pipeline run, so every path the
// driver can take is walked in seconds, without an API key, against a real
// checkout.
//
//   [[agents]]
//   name = "Mock pipeline"
//   command = "node"
//   args = ["crates/core/examples/mock_pipeline_agent.js"]
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
const SESSION = 'mock-pipeline';
const say = (text) =>
  send({
    jsonrpc: '2.0',
    method: 'session/update',
    params: {
      sessionId: SESSION,
      update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text } },
    },
  });

let cwd = process.cwd();
let turn = null; // { id, timers }
// The brief's orders, kept for the carry-on prompts that do not repeat it. A
// step's prompt carries the brief above onehand's `---`; only that part is read.
let orders = '';

const git = (...args) => execFileSync('git', args, { cwd, stdio: 'pipe' });

// What the prompt asks of this turn, and what the turn does about it.
function play(prompt) {
  if (prompt.includes('\n---\n')) orders = prompt.split('\n---\n')[0].toLowerCase();
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
        git('add', '-A');
        git('commit', '-q', '-m', 'mock pipeline change');
      }
    },
  };
}

function end(stopReason) {
  if (!turn) return;
  turn.timers.forEach(clearTimeout);
  send({ jsonrpc: '2.0', id: turn.id, result: { stopReason } });
  turn = null;
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
    case 'session/new':
      cwd = m.params?.cwd ?? cwd;
      return send({ jsonrpc: '2.0', id: m.id, result: { sessionId: SESSION } });
    case 'session/cancel':
      return end('cancelled');
    case 'session/prompt': {
      end('cancelled');
      const prompt = (m.params?.prompt ?? []).map((b) => b?.text ?? '').join('\n');
      const { lines, act } = play(prompt);
      const pace = orders.includes('fast') ? 0 : 6000 / (lines.length + 1);
      turn = { id: m.id, timers: [] };
      lines.forEach((text, i) => turn.timers.push(setTimeout(() => say(text), pace * i)));
      turn.timers.push(
        setTimeout(() => {
          try {
            act();
          } catch (e) {
            say(`\n\nThe mock could not touch the work: ${e.message}`);
          }
          end('end_turn');
        }, pace * (lines.length + 1)),
      );
      return;
    }
  }
});
