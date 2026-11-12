// The browser start of the e2e tests: one more try only for a browser that closed while it started.

import { describe, expect, it, vi } from 'vitest';
import { closedAtLaunch, launchWithRetry } from './launch';

const CLOSED = new Error('browserType.launch: Target page, context or browser has been closed\nBrowser logs: ...');

describe('the browser start of the e2e tests', () => {
  it('knows the error of a browser that closed at the start, and only that one', () => {
    expect(closedAtLaunch(CLOSED)).toBe(true);
    expect(closedAtLaunch(new Error('browserType.launch: Executable does not exist at C:/x/chrome.exe'))).toBe(false);
    expect(closedAtLaunch('Timeout 30000ms exceeded')).toBe(false);
  });

  it('starts once when the first start works', async () => {
    const launch = vi.fn(async () => 'browser');
    await expect(launchWithRetry(launch)).resolves.toBe('browser');
    expect(launch).toHaveBeenCalledTimes(1);
  });

  it('tries once more when the browser closed at the start, and says so', async () => {
    const launch = vi.fn().mockRejectedValueOnce(CLOSED).mockResolvedValueOnce('browser');
    const log = vi.fn();
    await expect(launchWithRetry(launch, log)).resolves.toBe('browser');
    expect(launch).toHaveBeenCalledTimes(2);
    expect(log).toHaveBeenCalledWith(expect.stringContaining('one more try'));
  });

  it('stays red for any other error, without a second start', async () => {
    const launch = vi.fn().mockRejectedValue(new Error('Executable does not exist'));
    await expect(launchWithRetry(launch)).rejects.toThrow('Executable does not exist');
    expect(launch).toHaveBeenCalledTimes(1);
  });

  it('stays red when the second start closes too', async () => {
    const launch = vi.fn().mockRejectedValue(CLOSED);
    await expect(launchWithRetry(launch)).rejects.toThrow('has been closed');
    expect(launch).toHaveBeenCalledTimes(2);
  });
});
