export interface Clock {
  last: number | null;
  carry: number;
}

export function makeClock(): Clock {
  return { last: null, carry: 0 };
}

export function due(clock: Clock, now: number, hidden: boolean, hz: number, cap: number, speed = 1): number {
  if (hidden || clock.last === null) {
    clock.last = hidden ? null : now;
    clock.carry = 0;
    return 0;
  }
  clock.carry += ((now - clock.last) / 1000) * hz * speed;
  clock.last = now;
  const whole = Math.floor(clock.carry);
  if (whole > cap) {
    clock.carry = 0;
    return cap;
  }
  clock.carry -= whole;
  return whole;
}
