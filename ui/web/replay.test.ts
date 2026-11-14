import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { hasEnd, lastPosition, stateAt, stopsOf } from '../src/lib/replay';
import { SCHEMA, statusOf } from '../src/lib/step';

// The recording a real run of testing.nvim left (`testing sheets step`, a sheet of four steps whose third fails), as
// the page holds it: the hub's form of an event has no path, and a picture is a yes.
const RECORDING = join(__dirname, '..', '..', 'crates', 'testing-hub', 'tests', 'fixtures', 'recording', 'events.jsonl');
const events: unknown[] = readFileSync(RECORDING, 'utf8')
  .split('\n')
  .filter((line) => line.trim() !== '')
  .map((line) => JSON.parse(line) as unknown);

describe('a recording played back', () => {
  it('rests before the first step, after each step that ran, and at the end', () => {
    const afters = events.map((e, i) => ((e as { event: string }).event === 'step-after' ? i : -1)).filter((i) => i >= 0);
    expect(afters).toHaveLength(3);
    expect(stopsOf(events)).toEqual([...afters, events.length]);
    expect(lastPosition(events)).toBe(3);
    expect(hasEnd(events)).toBe(true);
  });

  it('shows at the start the whole sheet with nothing run, and the first step as the next one', () => {
    const state = stateAt(events, 0);
    expect(state.doc?.id).toBe('demo.greeting');
    expect(state.doc?.steps.map((s) => s.n)).toEqual([1, 2, 3, 4]);
    expect(state.marks['80x24']).toEqual({});
    expect([1, 2, 3, 4].map((n) => statusOf(state, n))).toEqual(['next', 'pending', 'pending', 'pending']);
    expect(state.finished).toBeNull();
  });

  it('shows after two steps what they did, and nothing of what came later', () => {
    const state = stateAt(events, 2);
    expect([1, 2, 3, 4].map((n) => statusOf(state, n))).toEqual(['pass', 'pass', 'next', 'pending']);
    expect(state.marks['80x24']?.[1]?.screen?.[0]?.trimEnd()).toBe('hello from the sheet');
    // The note on step 3 was taken before it ran: it is there once the run stood before step 3.
    expect(state.notes['80x24']?.[3]).toEqual(['the cursor did move']);
    expect(state.finished).toBeNull();
  });

  it('shows at the end the failed step, the steps never reached and the verdict', () => {
    const state = stateAt(events, lastPosition(events));
    expect([1, 2, 3, 4].map((n) => statusOf(state, n))).toEqual(['pass', 'pass', 'fail', 'skipped']);
    expect(state.marks['80x24']?.[3]?.failure).toContain('got false');
    expect(state.finished).toMatchObject({ status: 'fail', ok: false, verdict: true });
  });

  it('waits for nobody: no pause, no question, no run under way at any position', () => {
    for (let p = 0; p <= lastPosition(events); p++) {
      const state = stateAt(events, p);
      expect([state.running, state.stepping, state.paused, state.question], `position ${p}`).toEqual([false, false, null, null]);
    }
  });

  it('keeps a position inside the recording', () => {
    expect(stateAt(events, -5)).toEqual(stateAt(events, 0));
    expect(stateAt(events, 99)).toEqual(stateAt(events, 3));
    expect(stateAt(events, 1.9)).toEqual(stateAt(events, 1));
  });

  it('a recording that was cut off ends where its lines end, without a verdict', () => {
    const cut = events.filter((e) => (e as { event: string }).event !== 'end');
    expect(hasEnd(cut)).toBe(false);
    const state = stateAt(cut, lastPosition(cut));
    expect(state.finished).toBeNull();
    expect(statusOf(state, 3)).toBe('fail');
  });

  it('an empty recording is one position with nothing in it', () => {
    expect(stopsOf([])).toEqual([0]);
    expect(stateAt([], 0).doc).toBeNull();
  });

  it('takes only lines of the contract', () => {
    const state = stateAt([{ schema: 'other/1', event: 'start', id: 'x' }, { schema: SCHEMA, event: 'unknown-later-event' }], 0);
    expect(state.doc).toBeNull();
  });
});
