#include <inttypes.h>
#include <stdio.h>

#include "feint.h"

int main(void) {
  const uint64_t seed = 77;
  const uint64_t cap = 240ull * 60 * 10;
  FeintGame *game = feint_game_new(seed, 1, 5);
  if (game == NULL) {
    return 1;
  }
  FeintBot *bots[2] = {feint_bot_new(1, seed, 0, game), feint_bot_new(5, seed, 1, game)};
  if (bots[0] == NULL || bots[1] == NULL) {
    return 1;
  }
  static uint8_t obs[FEINT_OBSERVATION_MAX];
  static uint8_t out[FEINT_ACTION_MAX];
  uint64_t tick = 0;
  while (feint_game_finished(game) == 0 && feint_game_tick(game, &tick) == 0 && tick < cap) {
    for (uint8_t seat = 0; seat < 2; seat++) {
      ptrdiff_t n = feint_game_observe(game, seat, obs, sizeof obs);
      if (n < 0) {
        return 2;
      }
      ptrdiff_t written = feint_bot_tick(bots[seat], game, obs, (size_t)n, out, sizeof out);
      if (written < 0) {
        return 3;
      }
      if (written > 0) {
        (void)feint_game_apply(game, out, (size_t)written);
      }
    }
    if (feint_game_advance(game) < 0) {
      return 4;
    }
  }
  float positions[FEINT_POSITIONS];
  float layout[FEINT_LAYOUT];
  if (feint_game_positions(game, positions, FEINT_POSITIONS) != FEINT_POSITIONS ||
      feint_game_layout(game, layout, FEINT_LAYOUT) != FEINT_LAYOUT || layout[0] <= 0.0f || layout[1] <= 0.0f ||
      positions[0] < -layout[0] - layout[3] || positions[0] > layout[0] + layout[3]) {
    return 6;
  }
  int64_t first = 0;
  int64_t second = 0;
  uint64_t digest = 0;
  if (feint_game_score(game, &first, &second) != 0 || feint_game_tick(game, &tick) != 0 ||
      feint_game_digest(game, &digest) != 0) {
    return 5;
  }
  printf("score %" PRId64 ":%" PRId64 " after %" PRIu64 " ticks digest %016" PRIx64 "\n", first, second, tick,
         digest);
  feint_bot_free(bots[0]);
  feint_bot_free(bots[1]);
  feint_game_free(game);
  return 0;
}
