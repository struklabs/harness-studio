// LaunchDialog previously kept model/folder choices in memory only. Persist this preference
// with the same best-effort localStorage convention as the other UI preferences.
const KEY = 'harness_opencode_hide_external_skills';
export function readHideExternalSkills(): boolean {
  try { return localStorage.getItem(KEY) !== 'false'; } catch { return true; }
}
export function saveHideExternalSkills(hide: boolean): void {
  try { localStorage.setItem(KEY, String(hide)); } catch { /* Storage unavailable. */ }
}
