#pragma once

#include <stddef.h>
#include <stdint.h>

#define FEINT_ERR_NULL -1

#define FEINT_ERR_INPUT -2

#define FEINT_ERR_SMALL -3

#define FEINT_ERR_BROKEN -4

#define FEINT_ERR_PANIC -5

#define FEINT_ACTION_MAX 256

#define FEINT_OBSERVATION_MAX 1024

typedef struct FeintBot FeintBot;

typedef struct FeintGame FeintGame;

const char *feint_version(void);

struct FeintGame *feint_game_new(uint64_t seed, uint8_t level_a, uint8_t level_b);

void feint_game_free(struct FeintGame *game);

ptrdiff_t feint_game_observe(struct FeintGame *game, uint8_t player, uint8_t *out, size_t cap);

ptrdiff_t feint_game_apply(struct FeintGame *game, const uint8_t *bytes, size_t len);

ptrdiff_t feint_game_advance(struct FeintGame *game);

ptrdiff_t feint_game_finished(const struct FeintGame *game);

ptrdiff_t feint_game_tick(const struct FeintGame *game, uint64_t *out);

ptrdiff_t feint_game_score(const struct FeintGame *game, int64_t *first, int64_t *second);

ptrdiff_t feint_game_digest(const struct FeintGame *game, uint64_t *out);

struct FeintBot *feint_bot_new(uint8_t level,
                               uint64_t seed,
                               uint8_t player,
                               const struct FeintGame *game);

void feint_bot_free(struct FeintBot *bot);

ptrdiff_t feint_bot_tick(struct FeintBot *bot,
                         const struct FeintGame *game,
                         const uint8_t *obs,
                         size_t len,
                         uint8_t *out,
                         size_t cap);
