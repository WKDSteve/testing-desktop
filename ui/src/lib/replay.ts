// A recorded stepped run played back: the events of one run (`events.jsonl` of testing.nvim, in the form the hub passes
// events on) and a position in them. The state at a position is the fold of `lib/step.ts` over the events up to there,
// so a recording reads exactly as the run did on the hub's page. Nothing here touches a transport or the DOM.

import { emptyStep, reduce, type StepState } from './step';

/**
 * Where a viewer can rest: `stops[p]` is how many events are folded at position `p`. Position `p` below the last is
 * "`p` steps ran": everything before the step-after of step `p + 1`. The last position is the whole recording.
 */
export function stopsOf(events: readonly unknown[]): number[] {
  const stops: number[] = [];
  events.forEach((event, index) => {
    if (typeof event === 'object' && event !== null && (event as { event?: unknown }).event === 'step-after') stops.push(index);
  });
  stops.push(events.length);
  return stops;
}

/** The last position of a recording. */
export function lastPosition(events: readonly unknown[]): number {
  return stopsOf(events).length - 1;
}

/**
 * The run as it stood at a position. Before the end nothing waits for a command and no question is open (a recording
 * takes none): the step that was about to run shows as the next one.
 */
export function stateAt(events: readonly unknown[], position: number): StepState {
  const stops = stopsOf(events);
  const last = stops.length - 1;
  const at = Math.min(Math.max(Math.trunc(position), 0), last);
  let state: StepState = { ...emptyStep(), session: 1 };
  for (const event of events.slice(0, stops[at])) state = reduce(state, event);
  state = { ...state, running: false, stepping: false, paused: null, question: null, error: null };
  return at === last ? state : { ...state, finished: null, exit: null };
}

/** Whether the recording holds the end of its run; one without was cut off (the run was killed). */
export function hasEnd(events: readonly unknown[]): boolean {
  return events.some((event) => typeof event === 'object' && event !== null && (event as { event?: unknown }).event === 'end');
}
