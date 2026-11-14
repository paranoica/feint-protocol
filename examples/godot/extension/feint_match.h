#ifndef FEINT_MATCH_H
#define FEINT_MATCH_H

#include <stddef.h>
#include <stdint.h>

#include "feint.h"

typedef struct {
  FeintGame *game;
  FeintBot *bots[2];
  uint8_t levels[2];
  uint64_t seed;
  float positions[FEINT_POSITIONS];
  float layout[FEINT_LAYOUT];
  uint8_t obs[FEINT_OBSERVATION_MAX];
  uint8_t out[FEINT_ACTION_MAX];
} Match;

void close_match(Match *m);
int start(Match *m, int64_t seed, int64_t level_a, int64_t level_b);
int64_t step(Match *m, double x, double y);
int64_t score(const Match *m, int64_t seat);
void play_out(Match *m, int64_t cap, char *text, size_t size);

#endif
