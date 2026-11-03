import { describe, expect, it } from 'vitest';
import fleetJson from '../../../crates/testing-hub/tests/fixtures/golden/fleet.json';
import repoJson from '../../../crates/testing-hub/tests/fixtures/golden/repo.json';
import type { Case, Fleet, RepoDoc, Row } from './contract';
import { caseTitle, notesOf, redCases, remoteTone, rowsOf, runLink, shown, verdictTone, viewOf } from './model';

const fleet = fleetJson as unknown as Fleet;
const repo = repoJson as unknown as RepoDoc;

function row(patch: Partial<Row>): Row {
  return { ...(fleet.repos[0] as Row), ...patch };
}

describe('the tones', () => {
  it('reads every state of the CI adapter, and what it does not know as unknown, never green', () => {
    expect(remoteTone('success')).toBe('green');
    for (const s of ['failure', 'rejected', 'cancelled'] as const) expect(remoteTone(s)).toBe('red');
    expect(remoteTone('running')).toBe('running');
    expect(remoteTone('queued')).toBe('running');
    expect(remoteTone('waiting_runner')).toBe('waiting');
    for (const s of ['skipped', 'no_run', 'no_github'] as const) expect(remoteTone(s)).toBe('skipped');
    expect(remoteTone('unknown')).toBe('unknown');
    expect(remoteTone(null)).toBe('unknown');
    expect(remoteTone('surprise' as never)).toBe('unknown');
  });

  it('takes a partial green for unknown, not for green', () => {
    expect(verdictTone('green')).toBe('green');
    expect(verdictTone('green-partial')).toBe('unknown');
    expect(verdictTone('red')).toBe('red');
    expect(verdictTone('infra')).toBe('red');
    expect(verdictTone(null)).toBe('unknown');
  });
});

describe('a link to the CI', () => {
  it('is built for a run or a job of a repository of the maintainer only', () => {
    for (const ok of [
      'https://github.com/WKDSteve/alpha.nvim/actions/runs/123',
      'https://github.com/WKDSteve/alpha.nvim/actions/runs/123/job/456',
    ]) {
      expect(runLink(ok)).toBe(ok);
    }
    for (const bad of [
      null,
      undefined,
      '',
      'javascript:alert(1)',
      'http://github.com/WKDSteve/alpha.nvim/actions/runs/1',
      'https://github.com.evil.example/WKDSteve/a/actions/runs/1',
      'https://github.com/Other/alpha.nvim/actions/runs/1',
      'https://github.com/WKDSteve/alpha.nvim/actions/runs/1?x=1',
      'https://github.com/WKDSteve/alpha.nvim/actions/runs/1#x',
      'https://github.com/WKDSteve/../x/actions/runs/1',
      'https://github.com/WKDSteve/a%2Fb/actions/runs/1',
      'https://github.com/WKDSteve/alpha.nvim/settings',
      'https://github.com/WKDSteve/alpha.nvim/actions/runs/1\n',
    ]) {
      expect(runLink(bad)).toBeNull();
    }
  });
});

describe('the rows of the fleet', () => {
  it('holds every repository of the golden fleet, red first', () => {
    const rows = rowsOf(fleet);
    expect(rows.map((r) => r.key).sort()).toEqual(fleet.repos.map((r) => r.key).sort());
    const firstGreen = rows.findIndex((r) => !r.red);
    expect(rows.slice(firstGreen).every((r) => !r.red)).toBe(true);
  });

  it('filters the red ones and a part of the name', () => {
    expect(rowsOf(fleet, { onlyRed: true }).every((r) => r.red)).toBe(true);
    expect(rowsOf(fleet, { query: 'ALPHA' }).map((r) => r.name)).toEqual(['alpha.nvim']);
    expect(rowsOf(fleet, { query: '  ' }).length).toBe(fleet.repos.length);
  });

  it('is red when the local run, the local CI or one system of the CI is red', () => {
    const green = row({ local: null, ci_local: null, ci_remote: null });
    expect(viewOf(green).red).toBe(false);
    const redCell = row({
      local: null,
      ci_local: null,
      ci_remote: {
        state: 'success',
        repo: null,
        sha: null,
        branch: null,
        workflow: null,
        os: { ubuntu: null, windows: { state: 'failure', url: null, job: null }, macos: null },
        run: null,
        note: null,
      },
    });
    expect(viewOf(redCell).red).toBe(true);
    expect(viewOf(redCell).ci.windows.tone).toBe('red');
    expect(viewOf(redCell).ci.ubuntu.tone).toBe('unknown');
  });

  it('says what the fleet hides: an empty fleet of unusable data classes is not green', () => {
    const notes = notesOf({ ...fleet, repos: [], classes: { checked: true, problem: 'no areas.json' } } as Fleet);
    expect(notes.classesProblem).toBe('no areas.json');
    expect(notesOf(fleet).localRead).toBe(true);
  });
});

describe('foreign text', () => {
  it('is shown with its hidden characters made visible', () => {
    // Made from their code points: the characters themselves are never in a source file.
    const rlo = String.fromCodePoint(0x202e);
    const zwsp = String.fromCodePoint(0x200b);
    expect(shown(`a${rlo}b`)).toBe('a⟦U+202E⟧b');
    expect(shown('plain')).toBe('plain');
    expect(shown(null)).toBeNull();
    const c = { ...(repo.cases[0] as Case), describe: [`x${zwsp}`], name: 'y' };
    expect(caseTitle(c)).toBe('x⟦U+200B⟧ › y');
  });

  it('the red cases of the golden repository are the ones that did not pass', () => {
    const red = redCases(repo.cases);
    expect(red.length).toBeGreaterThan(0);
    expect(red.every((c) => !['pass', 'skip', 'xfail'].includes(c.status))).toBe(true);
  });
});
