export interface GithubRelease {
  tag_name: string;
  name: string | null;
  body: string | null;
  html_url: string;
  published_at: string | null;
}

export interface UpdateCheckResult {
  hasUpdate: boolean;
  latest: string;
  release: GithubRelease | null;
  noReleases: boolean;
}

const RELEASE_LIST_URL =
  'https://api.github.com/repos/nice-buddy/easyJob/releases?per_page=1';

function normalizeVersion(version: string): number[] {
  const core = version.trim().replace(/^[vV]/, '').split('-')[0] ?? '';
  return core.split('.').map((part) => {
    const num = parseInt(part, 10);
    return Number.isNaN(num) ? 0 : num;
  });
}

export function isNewerVersion(current: string, latest: string): boolean {
  const currentParts = normalizeVersion(current);
  const latestParts = normalizeVersion(latest);
  const length = Math.max(currentParts.length, latestParts.length);
  for (let i = 0; i < length; i++) {
    const currentNum = currentParts[i] ?? 0;
    const latestNum = latestParts[i] ?? 0;
    if (latestNum > currentNum) return true;
    if (latestNum < currentNum) return false;
  }
  return false;
}

// 仓库尚未发布任何 Release 时返回 null（列表接口返回空数组，而非 404）。
export async function fetchLatestRelease(): Promise<GithubRelease | null> {
  let response: Response;
  try {
    response = await fetch(RELEASE_LIST_URL, {
      headers: { Accept: 'application/vnd.github+json' },
    });
  } catch {
    throw new Error('无法连接 GitHub，请检查网络后重试');
  }
  if (!response.ok) {
    throw new Error(`GitHub 查询失败（${response.status}），请稍后重试`);
  }
  const data = await response.json();
  if (!Array.isArray(data) || data.length === 0) {
    return null;
  }
  const item = data[0];
  return {
    tag_name: item.tag_name,
    name: item.name ?? null,
    body: item.body ?? null,
    html_url: item.html_url,
    published_at: item.published_at ?? null,
  };
}

export async function checkForUpdate(
  currentVersion: string,
): Promise<UpdateCheckResult> {
  const release = await fetchLatestRelease();
  if (!release) {
    return { hasUpdate: false, latest: '', release: null, noReleases: true };
  }
  return {
    hasUpdate: isNewerVersion(currentVersion, release.tag_name),
    latest: release.tag_name,
    release,
    noReleases: false,
  };
}
