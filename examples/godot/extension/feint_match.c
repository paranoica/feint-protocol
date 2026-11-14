#include <inttypes.h>
#include <math.h>
#include <stdio.h>

#include "feint_match.h"

#define BOUND 1000.0
#define LONGEST (240LL * 60 * 60)

void close_match(Match *m) {
  for (int s = 0; s < 2; s++) {
    if (m->bots[s] != NULL) {
      feint_bot_free(m->bots[s]);
      m->bots[s] = NULL;
    }
  }
  if (m->game != NULL) {
    feint_game_free(m->game);
    m->game = NULL;
  }
}

static int refresh(Match *m) {
  if (feint_game_positions(m->game, m->positions, FEINT_POSITIONS) != FEINT_POSITIONS) {
    return 0;
  }
  return feint_game_layout(m->game, m->layout, FEINT_LAYOUT) == FEINT_LAYOUT;
}

int start(Match *m, int64_t seed, int64_t level_a, int64_t level_b) {
  close_match(m);
  if (seed < 0 || level_a < 0 || level_a > 5 || level_b < 1 || level_b > 5) {
    return 0;
  }
  m->seed = (uint64_t)seed;
  m->levels[0] = (uint8_t)level_a;
  m->levels[1] = (uint8_t)level_b;
  m->game = feint_game_new(m->seed, m->levels[0], m->levels[1]);
  if (m->game == NULL) {
    return 0;
  }
  for (uint8_t s = 0; s < 2; s++) {
    if (m->levels[s] > 0) {
      m->bots[s] = feint_bot_new(m->levels[s], m->seed, s, m->game);
      if (m->bots[s] == NULL) {
        close_match(m);
        return 0;
      }
    }
  }
  if (!refresh(m)) {
    close_match(m);
    return 0;
  }
  return 1;
}

int64_t step(Match *m, double x, double y) {
  if (m->game == NULL) {
    return -1;
  }
  if (feint_game_finished(m->game) == 1) {
    return 1;
  }
  uint64_t tick = 0;
  if (feint_game_tick(m->game, &tick) != 0) {
    return -1;
  }
  for (uint8_t s = 0; s < 2; s++) {
    ptrdiff_t n = 0;
    if (m->bots[s] != NULL) {
      ptrdiff_t seen = feint_game_observe(m->game, s, m->obs, sizeof m->obs);
      if (seen < 0) {
        return -1;
      }
      n = feint_bot_tick(m->bots[s], m->game, m->obs, (size_t)seen, m->out, sizeof m->out);
    } else {
      double cx = (isfinite(x) && fabs(x) < BOUND) ? x : 0.0;
      double cy = (isfinite(y) && fabs(y) < BOUND) ? y : 0.0;
      float values[2] = {(float)cx, (float)cy};
      n = feint_action_encode(tick, s, values, 2, m->out, sizeof m->out);
    }
    if (n < 0) {
      return -1;
    }
    if (n > 0) {
      (void)feint_game_apply(m->game, m->out, (size_t)n);
    }
  }
  if (feint_game_advance(m->game) < 0 || !refresh(m)) {
    return -1;
  }
  return feint_game_finished(m->game) == 1;
}

int64_t score(const Match *m, int64_t seat) {
  int64_t first = 0;
  int64_t second = 0;
  if (m->game == NULL || seat < 0 || seat > 1 || feint_game_score(m->game, &first, &second) != 0) {
    return -1;
  }
  return seat == 0 ? first : second;
}

void play_out(Match *m, int64_t cap, char *text, size_t size) {
  snprintf(text, size, "error");
  if (m->game == NULL || m->bots[0] == NULL || m->bots[1] == NULL || cap <= 0 || cap > LONGEST) {
    return;
  }
  uint64_t tick = 0;
  while (feint_game_finished(m->game) == 0 && feint_game_tick(m->game, &tick) == 0 && tick < (uint64_t)cap) {
    if (step(m, 0.0, 0.0) < 0) {
      return;
    }
  }
  int64_t first = score(m, 0);
  int64_t second = score(m, 1);
  uint64_t digest = 0;
  if (first < 0 || second < 0 || feint_game_tick(m->game, &tick) != 0 || feint_game_digest(m->game, &digest) != 0) {
    return;
  }
  snprintf(text, size, "score %" PRId64 ":%" PRId64 " after %" PRIu64 " ticks digest %016" PRIx64, first, second,
           tick, digest);
}
