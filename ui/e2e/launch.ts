// Starting the browser of the e2e tests, with one more try when it closes while it starts. On a loaded runner
// Chrome sometimes goes away during the launch ("Target page, context or browser has been closed", CI run
// 43057454065); a second start then works. Any other error is a real one and stays red, and so does a second close.

/** The error of a browser that closed while it started; nothing else is tried again. */
export function closedAtLaunch(error: unknown): boolean {
  const text = error instanceof Error ? error.message : String(error);
  return /Target page, context or browser has been closed/i.test(text);
}

/** `launch()`, once more when the first one closed at the start. */
export async function launchWithRetry<T>(launch: () => Promise<T>, log: (line: string) => void = () => {}): Promise<T> {
  try {
    return await launch();
  } catch (error) {
    if (!closedAtLaunch(error)) throw error;
    log(`the browser closed while it started, one more try: ${String(error).split('\n')[0]}`);
    return await launch();
  }
}
