// A page in the background does not ask for anything: its loops (a run followed, a release watched, a hub that is asked
// again) wait here until the person looks at the page again.

/** What `whenVisible` needs of a document. */
export interface VisibleDocument {
  readonly hidden: boolean;
  addEventListener(type: 'visibilitychange', listener: () => void): void;
  removeEventListener(type: 'visibilitychange', listener: () => void): void;
}

/** Resolves at once while the document is in front, otherwise when it comes to the front. */
export function whenVisible(doc: VisibleDocument): Promise<void> {
  if (!doc.hidden) return Promise.resolve();
  return new Promise((resolve) => {
    const listener = (): void => {
      if (doc.hidden) return;
      doc.removeEventListener('visibilitychange', listener);
      resolve();
    };
    doc.addEventListener('visibilitychange', listener);
  });
}
