#!/usr/bin/env python3
"""Summarize payload-free batch/render traces and isolated echo latency.

--echo pairs each synthetic input with the last PTY batch before the next input,
then the next paint and GPUI submission. The reply counter is at the end of the
echo workload's output, so a frame painted before that tail is not an ack.
These are app-dispatch-to-submission timings, not physical key-to-photon latency.
"""
import argparse
import json
from pathlib import Path
import statistics


def summarize(path, echo=False):
    events = []
    for line in path.read_text().splitlines():
        if line.startswith('terminal_event '):
            event = dict(part.split('=', 1) for part in line.split()[1:])
            event['ns'] = int(event['ns'])
            event['bytes'] = int(event['bytes'])
            events.append(event)
    events.sort(key=lambda event: event['ns'])
    batches = [e for e in events if e['kind'] == 'batch']
    renders = [e['ns'] for e in events if e['kind'] == 'render']
    paints = [e['ns'] for e in events if e['kind'] == 'paint']
    presents = [e['ns'] for e in events if e['kind'] == 'present']
    tails = []
    for before, after in zip(batches, batches[1:]):
        between = [t for t in renders if before['ns'] < t < after['ns']]
        gap = (after['ns'] - before['ns']) / 1e6
        if between and before['bytes'] >= 4096 and after['bytes'] <= 2048 and gap < 25:
            tails.append({'first_batch_bytes': before['bytes'], 'tail_bytes': after['bytes'],
                          'gap_ms': gap, 'first_render_ms': (between[0] - before['ns']) / 1e6})
    result = {'batch_count': len(batches), 'render_count': len(renders),
              'short_tails_after_a_render': len(tails), 'tail_examples': tails[:8]}
    if echo:
        inputs = [e['ns'] for e in events if e['kind'] == 'input']
        samples = []
        for index, start in enumerate(inputs):
            limit = inputs[index + 1] if index + 1 < len(inputs) else float('inf')
            responses = [e for e in batches if start < e['ns'] < limit]
            paint = next((t for t in paints if responses and responses[-1]['ns'] < t < limit), None)
            present = next((t for t in presents if paint is not None and paint < t < limit), None)
            if present is not None:
                samples.append({'input': index, 'input_to_last_batch_ms': (responses[-1]['ns'] - start) / 1e6,
                                'input_to_paint_ms': (paint - start) / 1e6,
                                'input_to_submit_ms': (present - start) / 1e6})
        # The two child phases each acknowledge the same number of inputs.
        half = len(inputs) // 2
        result['input_count'] = len(inputs)
        result['matched_input_count'] = len(samples)
        result['echo_phases'] = {}
        for phase, first, last in [('echo', 0, half), ('echo-redraw', half, len(inputs))]:
            phase_samples = [s for s in samples if first <= s['input'] < last]
            summary = {'inputs': last - first, 'matched': len(phase_samples)}
            for key in ('input_to_last_batch_ms', 'input_to_paint_ms', 'input_to_submit_ms'):
                values = sorted(s[key] for s in phase_samples)
                if values:
                    summary[key] = {'median': statistics.median(values),
                                    'p95': values[max(0, (len(values) * 95 + 99) // 100 - 1)],
                                    'max': max(values)}
            result['echo_phases'][phase] = summary
        result['latency_samples'] = samples
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('log', type=Path)
    parser.add_argument('--echo', action='store_true')
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    text = json.dumps(summarize(args.log, args.echo), indent=2) + '\n'
    if args.output:
        args.output.write_text(text)
    else:
        print(text, end='')
