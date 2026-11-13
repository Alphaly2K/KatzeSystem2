#include <stddef.h>
#define PLM_NO_STDIO
#define PL_MPEG_IMPLEMENTATION
#include "pl_mpeg.h"

/* Keep the source length intact when metadata inspection seeks past EOF. */
void katze_movie_buffer_end(plm_buffer_t *buffer) {
    buffer->has_ended = TRUE;
}

/* A new pass starts with an empty MP2 synthesis filter history. */
void katze_movie_rewind(plm_t *player) {
    plm_rewind(player);
    if (player->audio_decoder) {
        player->audio_decoder->v_pos = 0;
        memset(player->audio_decoder->V, 0, sizeof(player->audio_decoder->V));
        memset(player->audio_decoder->U, 0, sizeof(player->audio_decoder->U));
    }
}
