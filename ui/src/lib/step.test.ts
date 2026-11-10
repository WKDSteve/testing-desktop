import { describe, expect, it } from 'vitest';
import { emptyStep, fromDocument, fromStream, isStale, notesOf, oneLine, reduce, screenRows, statusLine, statusOf, tileStep } from './step';

const SCHEMA = 'testing-sheet-session/1';
const ev = (event: string, fields: Record<string, unknown> = {}) => ({ schema: SCHEMA, event, ...fields });
const start = ev('start', {
  id: 'ui.float',
  title: ':UI option float',
  engine: 'rpc',
  sizes: [{ rows: 24, cols: 80 }],
  steps: [
    { n: 1, action: 'keys', value: ':UI<CR>', note: 'open the float' },
    { n: 2, action: 'expect', value: "vim.fn.winnr('$') == 2" },
    { n: 3, action: 'ask', value: 'Does it read well?' },
  ],
  headed: false,
  delay_ms: 0,
  breaks: [2],
});

describe('the model of a stepped run', () => {
  it('folds the events of a run into its state, as the panel does', () => {
    let s = reduce(emptyStep(), start);
    expect(s.doc?.title).toBe(':UI option float');
    expect(s.doc?.sizes).toEqual(['80x24']);
    expect(s.doc?.steps.map((st) => st.action)).toEqual(['keys', 'expect', 'ask']);
    expect(s.breaks).toEqual([2]);
    s = reduce(s, ev('size', { size: '80x24', state: 'open' }));
    expect(s.size).toBe('80x24');
    s = reduce(s, ev('paused', { size: '80x24', n: 1, reason: 'start' }));
    expect(statusLine(s)).toEqual({ kind: 'paused_before', n: 1, size: '80x24' });
    expect(statusOf(s, 1)).toBe('next');
    expect(statusOf(s, 2)).toBe('pending');
    s = reduce(s, ev('step-before', { size: '80x24', n: 1, total: 3, action: 'keys', value: ':UI<CR>' }));
    expect(statusOf(s, 1)).toBe('running');
    expect(statusLine(s)).toEqual({ kind: 'running', n: 1, size: '80x24' });
    s = reduce(s, ev('step-after', { size: '80x24', n: 1, status: 'pass', ms: 12, image: true, screen: ['~', 'float', '~', '~'] }));
    expect(statusOf(s, 1)).toBe('pass');
    expect(s.marks['80x24']?.[1]).toMatchObject({ status: 'pass', ms: 12, image: true, shot: false });
    s = reduce(s, ev('paused', { size: '80x24', n: 2, reason: 'break' }));
    s = reduce(s, ev('step-before', { size: '80x24', n: 2, total: 3, action: 'expect', value: 'x' }));
    s = reduce(s, ev('step-after', { size: '80x24', n: 2, status: 'fail', ms: 3, reason: 'not truthy', failure: '80x24 step 2: not truthy' }));
    s = reduce(s, ev('paused', { size: '80x24', n: 2, reason: 'failed' }));
    expect(statusLine(s)).toEqual({ kind: 'paused_failed', n: 2, size: '80x24' });
    expect(statusOf(s, 2)).toBe('fail');
    s = reduce(s, ev('end', { status: 'fail', ok: false, verdict: true, failures: ['80x24 step 2: not truthy'], snapshots: [], ms: 40 }));
    expect(s.finished).toMatchObject({ status: 'fail', ok: false, verdict: true, failures: ['80x24 step 2: not truthy'] });
    expect(s.running).toBe(false);
    expect(s.paused).toBeNull();
    expect(statusOf(s, 3)).toBe('skipped');
    expect(statusLine(s)).toEqual({ kind: 'finished', status: 'fail', ms: 40, skipped: null });
  });

  it('shows a question until its step is done, and keeps the last error', () => {
    let s = reduce(emptyStep(), start);
    s = reduce(s, ev('size', { size: '80x24', state: 'open' }));
    s = reduce(s, ev('step-before', { size: '80x24', n: 3, total: 3, action: 'ask', value: 'Does it read well?' }));
    s = reduce(s, ev('ask', { size: '80x24', n: 3, text: 'Does it read well?' }));
    expect(s.question).toEqual({ size: '80x24', n: 3, text: 'Does it read well?' });
    expect(statusLine(s)).toEqual({ kind: 'asks', text: 'Does it read well?' });
    s = reduce(s, ev('error', { message: 'this step waits for: answer yes|no [text]' }));
    expect(s.error).toBe('this step waits for: answer yes|no [text]');
    expect(s.question).not.toBeNull();
    s = reduce(s, ev('step-after', { size: '80x24', n: 3, status: 'fail', ms: 1, reason: 'too small' }));
    expect(s.question).toBeNull();
    expect(s.marks['80x24']?.[3]?.reason).toBe('too small');
  });

  it('keeps the notes of a run per step, in order, as the panel does', () => {
    let s = reduce(emptyStep(), start);
    s = reduce(s, ev('size', { size: '80x24', state: 'open' }));
    s = reduce(s, ev('note', { size: '80x24', n: 1, text: 'flickers once', time: '2026-11-10T10:00:00Z' }));
    s = reduce(s, ev('note', { size: '80x24', n: 1, text: 'and again', time: '2026-11-10T10:00:01Z' }));
    s = reduce(s, ev('note', { size: '80x24', text: 'no step' }));
    expect(notesOf(s, 1)).toEqual(['flickers once', 'and again']);
    expect(notesOf(s, 2)).toEqual([]);
    expect(notesOf(emptyStep(), 1)).toEqual([]);
    // A note on a step that has not run yet shows that step's tile, as the panel does.
    s = reduce(s, ev('step-after', { size: '80x24', n: 1, status: 'pass', ms: 1 }));
    s = reduce(s, ev('note', { size: '80x24', n: 2, text: 'next one', time: 't' }));
    expect(tileStep(s, null)).toBe(2);
    expect(tileStep(s, 2)).toBe(2);
  });

  it('refuses another schema and ignores what it does not know', () => {
    const s = reduce(emptyStep(), start);
    expect(reduce(s, { schema: 'testing-sheet-session/2', event: 'end', status: 'pass' })).toBe(s);
    expect(reduce(s, ev('something-new', { x: 1 }))).toBe(s);
    expect(reduce(s, 'not an object')).toBe(s);
    expect(reduce(s, ev('start', { steps: 'not a list', sizes: null })).doc?.steps).toEqual([]);
  });

  it("takes the hub's note of a child that ended without an end", () => {
    let s = reduce(emptyStep(), start);
    s = reduce(s, { event: 'exit', code: 2, detail: 'testing sheets: refused' });
    expect(s.exit).toEqual({ code: 2, detail: 'testing sheets: refused' });
    expect(s.running).toBe(false);
    expect(statusLine(s)).toEqual({ kind: 'exit', code: 2, detail: 'testing sheets: refused' });
    expect(statusOf(s, 1)).toBe('skipped');
  });

  it('reads the state of the hub, then only later lines of the same run from the stream', () => {
    const doc = {
      schema: 1,
      kind: 'hub.sheet_step',
      available: true,
      session: 4,
      running: true,
      seq: 3,
      key: 'alpha.nvim-0123456789ab',
      sheet: 'float',
      events: [start, ev('size', { size: '80x24', state: 'open' }), ev('paused', { size: '80x24', n: 1, reason: 'start' })],
    };
    let s = fromDocument(doc);
    expect(s.session).toBe(4);
    expect(s.seq).toBe(3);
    expect(s.running).toBe(true);
    expect(s.sheet).toBe('float');
    expect(s.at).toBe(1);
    // A line the state already holds changes nothing; a later one is taken, and the count follows it.
    const line = (session: number, seq: number, event: unknown) => JSON.stringify({ session, seq, event });
    const old = fromStream(s, line(4, 3, ev('paused', { size: '80x24', n: 9, reason: 'start' })));
    expect(old).toBe(s);
    s = fromStream(s, line(4, 4, ev('step-before', { size: '80x24', n: 1, total: 3, action: 'keys', value: 'x' })));
    expect(s.seq).toBe(4);
    expect(s.stepping).toBe(true);
    expect(isStale(s)).toBe(false);
    // Unreadable data changes nothing; a line of another run says: read the state again.
    expect(fromStream(s, '{not json')).toBe(s);
    const other = fromStream(s, line(5, 1, start));
    expect(isStale(other)).toBe(true);
    // A page that holds no run: the first line of a run is taken, a later one means it missed the start.
    expect(isStale(fromStream(emptyStep(), line(5, 1, start)))).toBe(false);
    expect(fromStream(emptyStep(), line(5, 1, start)).doc).not.toBeNull();
    expect(isStale(fromStream(emptyStep(), line(5, 7, ev('paused', { size: '80x24', n: 2, reason: 'step' }))))).toBe(true);
    // The end on the stream ends the run.
    s = fromStream(s, line(4, 5, ev('end', { status: 'pass', ok: true, verdict: true, failures: [], snapshots: [], ms: 9 })));
    expect(s.running).toBe(false);
  });

  it('says nothing runs for an empty state, and whether the hub can run at all', () => {
    const s = fromDocument({ schema: 1, kind: 'hub.sheet_step', available: false, session: null, running: false, seq: 0, key: null, sheet: null, events: [] });
    expect(s.available).toBe(false);
    expect(statusLine(s)).toEqual({ kind: 'none' });
    expect(fromDocument(null).available).toBe(true);
    expect(statusLine(fromDocument({ session: 1, running: true, seq: 0, events: [] }))).toEqual({ kind: 'starting' });
    // A document that says it runs but holds an end: it ended.
    const ended = fromDocument({ session: 1, running: true, seq: 2, events: [start, ev('end', { status: 'pass', ok: true, verdict: true, failures: [], snapshots: [], ms: 1 })] });
    expect(ended.running).toBe(false);
  });

  it('shows the tile of the chosen step, else of the last one that did something', () => {
    let s = reduce(emptyStep(), start);
    s = reduce(s, ev('size', { size: '80x24', state: 'open' }));
    s = reduce(s, ev('paused', { size: '80x24', n: 1, reason: 'start' }));
    expect(tileStep(s, null)).toBe(1);
    expect(tileStep(s, 3)).toBe(1);
    s = reduce(s, ev('step-after', { size: '80x24', n: 1, status: 'pass', ms: 1 }));
    s = reduce(s, ev('step-after', { size: '80x24', n: 2, status: 'pass', ms: 1 }));
    expect(tileStep(s, null)).toBe(2);
    expect(tileStep(s, 1)).toBe(1);
  });

  it('writes one line of a value and the rows of a screen as the panel does', () => {
    expect(oneLine('a  b\n\tc', 10)).toBe('a b c');
    expect(oneLine('abcdefghijkl', 8)).toBe('abcde...');
    expect(oneLine(42, 8)).toBe('42');
    expect(oneLine(null, 8)).toBe('');
    expect(screenRows(['hello   ', '~', '', '~  '])).toEqual(['hello']);
    // Hidden characters are made visible, as everywhere on the page.
    expect(oneLine(`a${String.fromCodePoint(0x202e)}b`, 20)).toContain('U+202E');
  });
});
