import { describe, expect, it } from 'vitest';
import { whenVisible, type VisibleDocument } from './visible';

function fakeDocument(hidden: boolean) {
  const listeners = new Set<() => void>();
  const doc: VisibleDocument & { hidden: boolean; change(hidden: boolean): void; listening(): number } = {
    hidden,
    addEventListener: (_type, listener) => void listeners.add(listener),
    removeEventListener: (_type, listener) => void listeners.delete(listener),
    change(next) {
      doc.hidden = next;
      for (const listener of [...listeners]) listener();
    },
    listening: () => listeners.size,
  };
  return doc;
}

describe('a page in the background waits to be looked at', () => {
  it('goes on at once while the page is in front', async () => {
    const doc = fakeDocument(false);
    await whenVisible(doc);
    expect(doc.listening()).toBe(0);
  });

  it('waits while it is hidden, goes on when it is shown, and leaves nothing behind', async () => {
    const doc = fakeDocument(true);
    let done = false;
    const waiting = whenVisible(doc).then(() => (done = true));
    await Promise.resolve();
    expect(done).toBe(false);
    doc.change(true);
    await Promise.resolve();
    expect(done).toBe(false);
    doc.change(false);
    await waiting;
    expect(done).toBe(true);
    expect(doc.listening()).toBe(0);
  });
});
