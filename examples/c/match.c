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
  while (feint_game_finished(game) == 0 && feint_game_tick(game) < cap) {
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
  int64_t score[2];
  if (feint_game_score(game, score) != 0) {
    return 5;
  }
  printf("score %" PRId64 ":%" PRId64 " after %" PRIu64 " ticks digest %016" PRIx64 "\n", score[0], score[1],
         feint_game_tick(game), feint_game_digest(game));
  feint_bot_free(bots[0]);
  feint_bot_free(bots[1]);
  feint_game_free(game);
  return 0;
}
