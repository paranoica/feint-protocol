export interface Ticket {
  ticket: string;
  seed: number;
}

export interface Profile {
  version: number;
  bytes: Uint8Array;
}

export function ticketFrom(json: unknown): Ticket | null {
  if (typeof json !== "object" || json === null) {
    return null;
  }
  const { ticket, seed } = json as Record<string, unknown>;
  const fits = typeof seed === "number" && Number.isInteger(seed) && seed >= 0 && seed <= 0xffffffff;
  if (typeof ticket !== "string" || !/^[0-9a-f]{16,64}$/.test(ticket) || !fits) {
    return null;
  }
  return { ticket, seed };
}

export function profileVersion(header: string | null): number | null {
  if (header === null || !/^[1-9][0-9]{0,15}$/.test(header)) {
    return null;
  }
  return Number(header);
}

export function profilePath(nick: string, game: string): string {
  return `/api/profile?nick=${encodeURIComponent(nick)}&game=${encodeURIComponent(game)}`;
}

export function uploadPath(nick: string, ticket: string, profile: number | null): string {
  const base = `/api/matches?nick=${encodeURIComponent(nick)}&ticket=${encodeURIComponent(ticket)}`;
  return profile === null ? base : `${base}&profile=${profile}`;
}

export function localSeed(random: () => number): number {
  return Math.floor(random() * 0x100000000) >>> 0;
}

export function seedRequest(): { method: string; headers: Record<string, string> } {
  return { method: "POST", headers: { "x-feint": "1" } };
}

export class Saves {
  private last: Promise<unknown> = Promise.resolve();

  track(work: Promise<unknown>): void {
    const quiet = work.catch(() => undefined);
    this.last = Promise.all([this.last, quiet]);
  }

  settled(capMs: number): Promise<boolean> {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const cap = new Promise<boolean>((resolve) => {
      timer = setTimeout(() => resolve(false), capMs);
    });
    const done = this.last.then(() => true);
    return Promise.race([done, cap]).finally(() => clearTimeout(timer));
  }
}
