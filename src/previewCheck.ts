import type { AccountView, TestResult } from "./types";

const BROWSER =
  "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";

export interface CheckInput {
  connectorId: string;
  username: string;
  timezone: string;
  leadHours: number;
  token?: string;
}

export async function checkPreview(input: CheckInput): Promise<TestResult & { view: Partial<AccountView> }> {
  const username = input.username.trim();
  if (!username) return fail("Enter the public username.");
  const zone = zoneFor(input.connectorId, input.timezone);
  try {
    const activity = await fetchActivity(input.connectorId, username, zone, input.token);
    const view = describe(activity, zone, input.leadHours);
    const summary =
      view.status === "ok"
        ? `${view.streak}-day streak · done today`
        : view.status === "ended"
          ? view.detail ?? "No current streak"
          : view.status === "idle"
            ? "No streak yet"
            : `${view.streak}-day streak · not done today`;
    return { ok: true, summary, view };
  } catch (err) {
    const message = err instanceof Error ? err.message : "Couldn't reach that site.";
    return fail(message);
  }
}

function fail(summary: string): TestResult & { view: Partial<AccountView> } {
  return {
    ok: false,
    summary,
    view: {
      status: "error",
      detail: summary,
      error: summary,
      streak: 0,
      doneToday: false,
      timeLeft: null,
      freshBreak: false,
      checkedAt: new Date().toISOString(),
    },
  };
}

interface Activity {
  days: string[];
  reported: number | null;
  lastActive: string | null;
  serviceUrl: string;
}

async function fetchActivity(
  connectorId: string,
  username: string,
  zone: string,
  token?: string,
): Promise<Activity> {
  if (connectorId === "duolingo") return fetchDuolingo(username);
  if (connectorId === "github") return fetchGithub(username, token);
  if (connectorId === "leetcode") return fetchLeetcode(username, zone);
  if (connectorId === "codeforces") return fetchCodeforces(username, zone);
  throw new Error("That connector isn't available.");
}

async function fetchDuolingo(username: string): Promise<Activity> {
  const body = await getJson(
    `/preview/duolingo/2017-06-30/users?username=${encodeURIComponent(username)}`,
  );
  const users = Array.isArray(body.users) ? body.users : [];
  const user =
    users.find((item) => String(item.username ?? "").toLowerCase() === username.toLowerCase()) ??
    users[0];
  if (!user) throw new Error(`No Duolingo profile named ${username}.`);
  const current = user.streakData?.currentStreak;
  if (current == null) {
    const streak = Number(user.streak ?? 0);
    if (!streak) {
      return { days: [], reported: 0, lastActive: null, serviceUrl: profile("duolingo", username) };
    }
  }
  const length = Number(current?.length ?? user.streak ?? 0);
  const lastActive = typeof current?.endDate === "string" ? current.endDate.slice(0, 10) : null;
  if (length > 0 && !lastActive) {
    throw new Error("Duolingo didn't include the day this streak was last extended.");
  }
  return { days: [], reported: length, lastActive, serviceUrl: profile("duolingo", username) };
}

async function fetchGithub(username: string, token?: string): Promise<Activity> {
  const days = token?.trim()
    ? await githubGraphql(username, token.trim())
    : await githubHtml(username);
  return { days, reported: null, lastActive: null, serviceUrl: profile("github", username) };
}

async function githubHtml(username: string): Promise<string[]> {
  const response = await fetch(`/preview/github/users/${encodeURIComponent(username)}/contributions`, {
    headers: { Accept: "text/html", "User-Agent": BROWSER },
  });
  if (!response.ok) throw new Error(`GitHub responded with ${response.status}.`);
  const html = await response.text();
  const days: string[] = [];
  let found = 0;
  for (const chunk of html.split("<td").slice(1)) {
    const cell = chunk.slice(0, 800);
    const date = attr(cell, "data-date");
    const level = attr(cell, "data-level");
    if (!date || level == null) continue;
    found += 1;
    if (Number(level) > 0) days.push(date);
  }
  if (found === 0) throw new Error("GitHub's page didn't include a contribution calendar.");
  return unique(days);
}

async function githubGraphql(username: string, token: string): Promise<string[]> {
  const now = new Date();
  const from = new Date(now.getTime() - 400 * 86400000).toISOString();
  const to = new Date(now.getTime() + 86400000).toISOString();
  const response = await fetch("/preview/github-api/graphql", {
    method: "POST",
    headers: {
      Authorization: `Bearer ${token}`,
      Accept: "application/vnd.github+json",
      "Content-Type": "application/json",
      "User-Agent": "streak-orb",
    },
    body: JSON.stringify({
      query:
        "query($login: String!, $from: DateTime!, $to: DateTime!) { user(login: $login) { contributionsCollection(from: $from, to: $to) { contributionCalendar { weeks { contributionDays { date contributionCount } } } } } }",
      variables: { login: username, from, to },
    }),
  });
  const body = await response.json();
  if (response.status === 401 || String(body.message ?? "").toLowerCase().includes("bad credentials")) {
    throw new Error("GitHub didn't accept that token.");
  }
  if (body.data?.user == null) throw new Error("No GitHub user with that name.");
  const weeks = body.data.user.contributionsCollection?.contributionCalendar?.weeks ?? [];
  const days: string[] = [];
  for (const week of weeks) {
    for (const cell of week.contributionDays ?? []) {
      if (Number(cell.contributionCount) > 0 && cell.date) days.push(cell.date);
    }
  }
  return unique(days);
}

async function fetchLeetcode(username: string, zone: string): Promise<Activity> {
  const response = await fetch("/preview/leetcode/graphql", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Referer: `https://leetcode.com/${username}/`,
      "User-Agent": BROWSER,
    },
    body: JSON.stringify({
      query:
        "query userCalendar($username: String!) { matchedUser(username: $username) { userCalendar { streak submissionCalendar } } }",
      variables: { username },
    }),
  });
  if (!response.ok) throw new Error(`LeetCode responded with ${response.status}.`);
  const body = await response.json();
  if (body.data?.matchedUser == null) throw new Error(`No LeetCode user named ${username}.`);
  const raw = body.data.matchedUser.userCalendar?.submissionCalendar;
  const map = typeof raw === "string" ? JSON.parse(raw) : raw ?? {};
  const days: string[] = [];
  for (const [key, count] of Object.entries(map)) {
    if (Number(count) <= 0) continue;
    const day = dateFromUnix(Number(key), zone);
    if (day) days.push(day);
  }
  return { days: unique(days), reported: null, lastActive: null, serviceUrl: profile("leetcode", username) };
}

async function fetchCodeforces(username: string, zone: string): Promise<Activity> {
  const days: string[] = [];
  let from = 1;
  const today = todayIn(zone);
  for (let page = 0; page < 6; page += 1) {
    const body = await getJson(
      `/preview/codeforces/api/user.status?handle=${encodeURIComponent(username)}&from=${from}&count=100`,
    );
    if (body.status !== "OK") {
      const comment = String(body.comment ?? "Codeforces couldn't look up that handle.");
      throw new Error(comment.toLowerCase().includes("not found") ? "No Codeforces user with that handle." : comment);
    }
    const rows = Array.isArray(body.result) ? body.result : [];
    let oldest: string | null = null;
    for (const row of rows) {
      const day = dateFromUnix(Number(row.creationTimeSeconds), zone);
      if (!day) continue;
      if (!oldest || day < oldest) oldest = day;
      if (row.verdict === "OK") days.push(day);
    }
    const far = oldest != null && daySpan(oldest, today) > 400;
    if (rows.length < 100 || far) break;
    from += 100;
  }
  return {
    days: unique(days),
    reported: null,
    lastActive: null,
    serviceUrl: profile("codeforces", username),
  };
}

function describe(activity: Activity, zone: string, leadHours: number): Partial<AccountView> {
  const today = todayIn(zone);
  const yesterday = shift(today, -1);
  const inLead = inLeadWindow(zone, leadHours);
  const days = new Set(activity.days);
  const last = activity.lastActive ?? [...days].sort().at(-1) ?? null;
  let status = "idle";
  let streak = 0;
  let doneToday = false;
  let detail = "No streak yet";

  if (activity.reported != null) {
    const alive = last === today || last === yesterday || (last == null && activity.reported > 0);
    doneToday = last === today;
    if (!alive) {
      status = last ? "ended" : "idle";
      detail = last ? (fresh(last, today) ? "Streak ended" : "No current streak") : "No streak yet";
    } else {
      streak = activity.reported;
      status = doneToday ? "ok" : inLead ? "at_risk" : "waiting";
      detail = doneToday ? "Done today" : "Still open today";
    }
  } else {
    doneToday = days.has(today);
    const start = doneToday ? today : days.has(yesterday) ? yesterday : null;
    if (start) {
      let cursor: string | null = start;
      while (cursor && days.has(cursor)) {
        streak += 1;
        cursor = shift(cursor, -1);
      }
    }
    if (streak > 0 && doneToday) status = "ok";
    else if (streak > 0 && inLead) status = "at_risk";
    else if (streak > 0) status = "waiting";
    else if (last) status = "ended";
    detail =
      status === "ok"
        ? "Done today"
        : status === "waiting" || status === "at_risk"
          ? "Still open today"
          : status === "ended"
            ? fresh(last, today)
              ? "Streak ended"
              : "No current streak"
            : "No streak yet";
  }

  const mins = minutesUntilMidnight(zone);
  return {
    streak,
    doneToday,
    status,
    detail,
    error: null,
    checkedAt: new Date().toISOString(),
    serviceUrl: activity.serviceUrl,
    timeLeft: status === "at_risk" || status === "waiting" ? formatLeft(mins) : null,
    freshBreak: status === "ended" && fresh(last, today),
  };
}

function zoneFor(connectorId: string, timezone: string): string {
  const trimmed = timezone.trim();
  if (trimmed) return trimmed;
  if (connectorId === "leetcode") return "UTC";
  return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
}

function todayIn(zone: string): string {
  try {
    return new Intl.DateTimeFormat("en-CA", { timeZone: zone, year: "numeric", month: "2-digit", day: "2-digit" }).format(
      new Date(),
    );
  } catch {
    throw new Error(`Unknown time zone "${zone}". Try UTC or America/New_York.`);
  }
}

function inLeadWindow(zone: string, leadHours: number): boolean {
  if (leadHours <= 0) return false;
  if (leadHours >= 24) return true;
  const parts = new Intl.DateTimeFormat("en-GB", {
    timeZone: zone,
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
  }).formatToParts(new Date());
  const hour = Number(parts.find((part) => part.type === "hour")?.value ?? 0);
  const minute = Number(parts.find((part) => part.type === "minute")?.value ?? 0);
  return hour * 60 + minute >= (24 - leadHours) * 60;
}

function minutesUntilMidnight(zone: string): number {
  const parts = new Intl.DateTimeFormat("en-GB", {
    timeZone: zone,
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
  }).formatToParts(new Date());
  const hour = Number(parts.find((part) => part.type === "hour")?.value ?? 0);
  const minute = Number(parts.find((part) => part.type === "minute")?.value ?? 0);
  return Math.max(0, (24 * 60) - (hour * 60 + minute));
}

function formatLeft(mins: number): string {
  if (mins < 1) return "under a minute left";
  if (mins < 60) return `${mins} min left`;
  const hours = Math.ceil(mins / 60);
  return hours === 1 ? "1 hour left" : `${hours} hours left`;
}

function shift(ymd: string, delta: number): string {
  const [year, month, day] = ymd.split("-").map(Number);
  const next = new Date(Date.UTC(year, month - 1, day + delta));
  return next.toISOString().slice(0, 10);
}

function daySpan(from: string, to: string): number {
  const start = Date.parse(`${from}T00:00:00Z`);
  const end = Date.parse(`${to}T00:00:00Z`);
  return Math.round((end - start) / 86400000);
}

function fresh(last: string | null, today: string): boolean {
  if (!last) return false;
  const age = daySpan(last, today);
  return age >= 1 && age <= 7;
}

function dateFromUnix(seconds: number, zone: string): string | null {
  if (!Number.isFinite(seconds)) return null;
  const ms = Math.abs(seconds) > 10_000_000_000 ? seconds : seconds * 1000;
  try {
    return new Intl.DateTimeFormat("en-CA", {
      timeZone: zone,
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
    }).format(new Date(ms));
  } catch {
    return null;
  }
}

function profile(connectorId: string, username: string): string {
  if (connectorId === "duolingo") return `https://www.duolingo.com/profile/${username}`;
  if (connectorId === "github") return `https://github.com/${username}`;
  if (connectorId === "leetcode") return `https://leetcode.com/${username}`;
  return `https://codeforces.com/profile/${username}`;
}

function attr(tag: string, name: string): string | null {
  const key = `${name}="`;
  const start = tag.indexOf(key);
  if (start < 0) return null;
  const rest = tag.slice(start + key.length);
  const end = rest.indexOf('"');
  return end < 0 ? null : rest.slice(0, end);
}

function unique(days: string[]): string[] {
  return [...new Set(days)].sort();
}

async function getJson(url: string): Promise<{
  users?: Array<{ username?: string; streak?: number; streakData?: { currentStreak?: { length?: number; endDate?: string } | null } }>;
  status?: string;
  comment?: string;
  result?: Array<{ verdict?: string; creationTimeSeconds?: number }>;
}> {
  const response = await fetch(url, {
    headers: { Accept: "application/json", "User-Agent": BROWSER },
  });
  const text = await response.text();
  try {
    return JSON.parse(text);
  } catch {
    throw new Error(response.ok ? "The site returned something that wasn't JSON." : `The site responded with ${response.status}.`);
  }
}
