import { describe, expect, it } from 'vitest';
import fleetJson from '../../../crates/testing-hub/tests/fixtures/golden/fleet.json';
import repoJson from '../../../crates/testing-hub/tests/fixtures/golden/repo.json';
import findingsJson from '../../../crates/testing-hub/tests/fixtures/golden/findings.json';
import type { Case, Fleet, Metrics, RepoDoc, Row } from './contract';
import { textsOf } from './i18n';
import { ageOf, caseTitle, ciTip, findingsOf, greenPercent, localTip, metricsSummary, notesOf, redCases, remoteTone, rowsOf, runLink, runTone, shown, verdictTone, viewOf } from './model';

const fleet = fleetJson as unknown as Fleet;
const repo = repoJson as unknown as RepoDoc;

function row(patch: Partial<Row>): Row {
  return { ...(fleet.repos[0] as Row), ...patch };
}

describe('the tones', () => {
  it('reads every state of the CI adapter, and what it does not know as unknown, never green', () => {
    expect(remoteTone('success')).toBe('green');
    for (const s of ['failure', 'rejected', 'cancelled', 'no_runner'] as const) expect(remoteTone(s)).toBe('red');
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

describe('the findings', () => {
  it('reads the findings of the golden document in their order', () => {
    const list = findingsOf(findingsJson);
    expect(list.map((f) => f.kind)).toEqual(expect.arrayContaining(['case', 'flaky']));
    expect(list[0]).toMatchObject({ fp: '0131da64ae656bac', title: 'fail: reads a file', where: { file: 'TESTS/x_spec.lua', line: 12 } });
  });

  it('leaves out what is no finding, and reads a field of another type as nothing', () => {
    const list = findingsOf({
      findings: [null, 'x', [], {}, { fp: 7 }, { fp: 'ok', kind: 5, title: {}, excerpt: null, where: { file: 3, line: 'x' }, run: 'no' }],
    });
    expect(list).toEqual([{ fp: 'ok', kind: 'unknown', title: '', excerpt: '', where: null, repro: null, rule: null, run: null }]);
    expect(findingsOf({})).toEqual([]);
    expect(findingsOf({ findings: 'x' })).toEqual([]);
  });

  it('makes a link only of a CI run of GitHub', () => {
    const good = 'https://github.com/WKDSteve/testing.nvim/actions/runs/123';
    const [a, b] = findingsOf({
      findings: [
        { fp: 'a', run: { source: 'ci', os: 'ubuntu', url: good } },
        { fp: 'b', run: { source: 'ci', os: null, url: 'javascript:alert(1)' } },
      ],
    });
    expect(a?.run).toEqual({ source: 'ci', os: 'ubuntu', link: good });
    expect(b?.run).toEqual({ source: 'ci', os: null, link: null });
  });
});

describe('the log of the runs', () => {
  it('reads a verdict of a run, and what it does not know as unknown, never green', () => {
    expect(runTone('green')).toBe('green');
    expect(runTone('green-partial')).toBe('green');
    expect(runTone('red')).toBe('red');
    expect(runTone('infra')).toBe('red');
    expect(runTone('whatever')).toBe('unknown');
  });

  it('says a share only when there are runs with a result', () => {
    expect(greenPercent(3, 1)).toBe(75);
    expect(greenPercent(2, 1)).toBe(67);
    expect(greenPercent(0, 0)).toBeNull();
    expect(greenPercent(0, 2)).toBe(0);
  });

  it('sums up the cost of the runs of the golden repository', () => {
    const m = repo.metrics as Metrics;
    expect(metricsSummary(m)).toEqual({
      runs: 5,
      minutes: 17,
      waitMinutes: 0.5,
      waited: 1,
      quickShare: 0,
      perPush: 5,
      pushes: 1,
      hitRate: 80,
    });
  });

  it('says the share of quick commit checks only when there was one', () => {
    const kind = (k: Metrics['kinds'][number]['kind'], runs: number) => ({
      kind: k,
      runs,
      minutes: 1,
      median_s: 1,
      wait_minutes: 0.25,
      waited: 1,
      verdicts: [],
    });
    const base: Metrics = {
      runs: 4,
      minutes: 4,
      first: '2026-11-01T10:00:00Z',
      last: '2026-11-02T10:00:00Z',
      backfilled: 0,
      kinds: [kind('ci-commit', 1), kind('ci-commit-quick', 3)],
      days: [],
      cache: { selected: 0, cached: 0, ran: 0, hit_rate: null },
      pushes: null,
      runs_per_push: null,
    };
    expect(metricsSummary(base).quickShare).toBe(75);
    expect(metricsSummary(base).waitMinutes).toBe(0.5);
    expect(metricsSummary(base).perPush).toBeNull();
    expect(metricsSummary(base).hitRate).toBeNull();
    expect(metricsSummary({ ...base, kinds: [kind('suite', 4)] }).quickShare).toBeNull();
  });
});

describe('the age of an export', () => {
  const NOW = Date.parse('2026-11-05T12:00:00Z');
  it('says how long ago, and nothing for what is no time or lies ahead', () => {
    expect(ageOf('2026-11-05T11:59:30Z', NOW)).toEqual({ unit: 'now', n: 0 });
    expect(ageOf('2026-11-05T11:15:00Z', NOW)).toEqual({ unit: 'minutes', n: 45 });
    expect(ageOf('2026-11-05T09:00:00Z', NOW)).toEqual({ unit: 'hours', n: 3 });
    expect(ageOf('2026-11-01T12:00:00Z', NOW)).toEqual({ unit: 'days', n: 4 });
    expect(ageOf('never', NOW)).toBeNull();
    expect(ageOf('2026-11-05T13:00:00Z', NOW)).toBeNull();
  });
});

describe('the sentence of a cell', () => {
  const views = Object.fromEntries(rowsOf(fleet).map((v) => [v.name, v]));
  const de = textsOf('de');
  const say = (tip: { key: Parameters<typeof de>[0]; params: Record<string, string | number> }) => de(tip.key, tip.params);

  it('names the repository and the system, and says what the word in the cell means', () => {
    expect(say(ciTip(views['alpha.nvim']!, 'ubuntu'))).toBe('Der letzte CI-Lauf von alpha.nvim auf ubuntu war grün.');
    expect(say(ciTip(views['beta.nvim']!, 'windows'))).toBe('Der letzte CI-Lauf von beta.nvim auf windows ist fehlgeschlagen.');
    expect(say(localTip(views['alpha.nvim']!))).toBe('Der letzte lokale Lauf von alpha.nvim war grün.');
    expect(say(localTip(views['gamma.nvim']!))).toBe('Auf diesem Rechner ist kein Lauf von gamma.nvim gespeichert.');
  });

  it('counts the failed cases of a red local run', () => {
    const tip = localTip(views['beta.nvim']!);
    expect(tip.key).toBe('tip.local.failed');
    expect(tip.params['n']).toBe(views['beta.nvim']!.local!.failed);
    expect(localTip({ ...views['beta.nvim']!, local: { tone: 'red', verdict: 'red', failed: 0, ts: null } }).key).toBe('tip.local.red');
  });

  it('does not call a part of the suite or an unknown verdict green', () => {
    const base = views['alpha.nvim']!;
    expect(localTip(viewOf(row({ local: { ...row({}).local!, verdict: 'green-partial' } }))).key).toBe('tip.local.partial');
    expect(localTip({ ...base, local: { tone: 'unknown', verdict: 'surprise', failed: 0, ts: null } }).key).toBe('tip.local.unknown');
  });

  it('explains a system without a job by the state of the repository, else by the missing job', () => {
    expect(ciTip(views['gamma.nvim']!, 'macos').key).toBe('tip.ci.no_github');
    expect(ciTip(views['beta.nvim']!, 'macos').key).toBe('tip.ci.no_job');
  });

  it('says that GitHub was not asked when the row has no CI state at all', () => {
    expect(ciTip(viewOf(row({ ci_remote: null })), 'ubuntu').key).toBe('tip.ci.not_asked');
  });

  it('has a sentence for every state of the adapter, and reads a state it does not know as unknown', () => {
    const base = views['alpha.nvim']!;
    const states = ['failure', 'rejected', 'cancelled', 'no_runner', 'unknown', 'waiting_runner', 'queued', 'running', 'success', 'skipped', 'no_run', 'no_github'] as const;
    for (const state of states) {
      const tip = ciTip({ ...base, ci: { ...base.ci, ubuntu: { tone: remoteTone(state), state, link: null } } }, 'ubuntu');
      expect(tip.key).toBe(`tip.ci.${state}`);
      expect(say(tip), state).toContain('alpha.nvim');
    }
    const odd = ciTip({ ...base, ci: { ...base.ci, ubuntu: { tone: 'unknown', state: 'surprise' as never, link: null } } }, 'ubuntu');
    expect(odd.key).toBe('tip.ci.unknown');
  });

  it('shows hidden characters of a name instead of obeying them', () => {
    const tip = ciTip(viewOf(row({ name: 'a\u202eb' })), 'ubuntu');
    expect(tip.params['repo']).toBe(shown('a\u202eb'));
    expect(tip.params['repo']).not.toContain('\u202e');
  });
});
