import { describe, it, expect, vi, afterEach } from 'vitest';
import { isNewerVersion, fetchLatestRelease, checkForUpdate } from '../src/services/update';

afterEach(() => {
  vi.unstubAllGlobals();
});

const releaseV102 = {
  tag_name: 'v1.0.2',
  name: 'easyJob v1.0.2',
  body: 'Bug fixes',
  html_url: 'https://github.com/nice-buddy/easyJob/releases/tag/v1.0.2',
  published_at: '2026-09-20T00:00:00Z',
};

const releaseV101 = {
  tag_name: 'v1.0.1',
  name: 'easyJob v1.0.1',
  body: '',
  html_url: 'https://github.com/nice-buddy/easyJob/releases/tag/v1.0.1',
  published_at: '2026-09-20T00:00:00Z',
};

function mockReleaseList(payload: unknown, ok = true, status = 200) {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
    ok,
    status,
    json: async () => payload,
  }));
}

describe('update service', () => {
  it('detects a newer release version', () => {
    expect(isNewerVersion('1.0.1', '1.0.2')).toBe(true);
  });

  it('returns false when versions are equal (ignores leading v)', () => {
    expect(isNewerVersion('v1.0.1', '1.0.1')).toBe(false);
  });

  it('compares numerically, not lexicographically', () => {
    expect(isNewerVersion('1.0.9', '1.0.10')).toBe(true);
    expect(isNewerVersion('1.0.10', '1.0.9')).toBe(false);
  });

  it('detects patch-level updates', () => {
    expect(isNewerVersion('1.0.1', '1.0.2')).toBe(true);
  });

  it('fetches the latest release from the GitHub release list', async () => {
    mockReleaseList([releaseV102]);

    const release = await fetchLatestRelease();
    expect(release?.tag_name).toBe('v1.0.2');
    expect(release?.html_url).toContain('releases/tag/v1.0.2');
  });

  it('returns null when no releases have been published yet', async () => {
    mockReleaseList([]);

    await expect(fetchLatestRelease()).resolves.toBeNull();
  });

  it('throws a readable error when the release API fails', async () => {
    mockReleaseList({}, false, 403);

    await expect(fetchLatestRelease()).rejects.toThrow();
  });

  it('reports hasUpdate=true when the release is newer', async () => {
    mockReleaseList([releaseV102]);

    const result = await checkForUpdate('1.0.1');
    expect(result.hasUpdate).toBe(true);
    expect(result.latest).toBe('v1.0.2');
    expect(result.noReleases).toBe(false);
  });

  it('reports hasUpdate=false when already on the latest version', async () => {
    mockReleaseList([releaseV101]);

    const result = await checkForUpdate('1.0.1');
    expect(result.hasUpdate).toBe(false);
    expect(result.noReleases).toBe(false);
  });

  it('reports noReleases=true instead of erroring when nothing is published', async () => {
    mockReleaseList([]);

    const result = await checkForUpdate('1.0.1');
    expect(result.noReleases).toBe(true);
    expect(result.hasUpdate).toBe(false);
  });
});
