use super::{Opening, SIDE_SIZE, TAKEN};
use crate::{Error, Result};

const REVEALED: u32 = 0x10000;
const CLAIMED: u32 = 4;

impl Opening {
    pub(in crate::kcs::mahjong) fn discards(&self, seat: usize) -> &[u32] {
        &self.discards[seat]
    }

    pub(in crate::kcs::mahjong) fn select_discard(&mut self, index: i32) {
        if !matches!(self.phase, 20043 | 20020) {
            return;
        }
        let hand_len = self.hands[self.seat as usize].len();
        if hand_len != 0 && index < hand_len as i32 {
            self.discard_index = if index < 0 {
                hand_len - 1
            } else {
                index as usize
            };
            self.phase = 20017;
        }
    }

    pub(in crate::kcs::mahjong) fn select_chi(&mut self, index: u32) {
        if self.phase != 20045 {
            return;
        }
        let Some(&choice) = self.chi_choices.get(index as usize) else {
            return;
        };
        self.selected_chi = Some(choice);
        self.phase = 20040;
    }

    pub(super) fn poll_turn(
        &mut self,
        packet: &mut Vec<u32>,
        points: [u32; 4],
        remaining: &mut u32,
        seats: usize,
        pc: usize,
    ) -> Result<u32> {
        let mut event = self.phase;
        let seat = self.seat as usize;
        self.phase = match self.phase {
            20007 => {
                packet.push(self.seat);
                20008
            }
            20008 => {
                let hand = &mut self.hands[seat];
                hand.retain(|tile| tile & TAKEN == 0);
                hand.sort_by_key(|tile| tile & 0x3ff);
                packet.push(self.seat);
                if self.selected_chi.take().is_some() {
                    20042
                } else {
                    20021
                }
            }
            20009 => 20010,
            20010 => {
                let tile = self.draw_order[130];
                let position = ((tile / SIDE_SIZE) * 256 + tile % SIDE_SIZE) as u32;
                packet.extend([self.wall[tile], position, position]);
                self.wall[tile] |= REVEALED;
                20019
            }
            20017 => 20018,
            20018 => {
                let discard = &mut self.hands[seat][self.discard_index];
                let pile = &mut self.discards[seat];
                if pile.len() >= 32 {
                    return Err(Error::new(pc, "麻将弃牌超过表容量"));
                }
                packet.extend([
                    *discard,
                    0x2_0000 | (self.seat << 8) | self.discard_index as u32,
                    REVEALED | (self.seat << 8) | pile.len() as u32,
                ]);
                pile.push(*discard | REVEALED);
                *discard |= TAKEN;
                20007
            }
            20019 => 20020,
            20020 => {
                let can_declare_ready = self.meld_count[seat] == 0
                    && (points[seat] as i32) >= 1000
                    && *remaining as usize >= seats;
                if self.needs_special_action(seat, *remaining != 0, can_declare_ready) {
                    return Err(Error::new(pc, "麻将出牌前的特殊选项尚未接入"));
                }
                event = 20042;
                packet[0] = event;
                20043
            }
            20021 | 20045 => {
                let next = (seat + 1) % seats;
                if self.phase == 20021 {
                    let discard = *self.discards[seat].last().unwrap();
                    if self.has_other_claim(seat, seats, *remaining, tile_kind(discard)) {
                        return Err(Error::new(pc, "麻将碰、杠及和牌响应尚未接入"));
                    }
                    if next == 0 && *remaining != 0 {
                        self.chi_choices = chi_choices(&self.hands[next], discard);
                    }
                    if !self.chi_choices.is_empty() {
                        if self.meld_count[next] != 0 {
                            return Err(Error::new(pc, "麻将多组副露的吃牌尚未接入"));
                        }
                        packet.push(0x40000);
                        packet.extend(
                            self.chi_choices
                                .iter()
                                .map(|&[i, j]| (0x40 | j as u32) << 12 | (i as u32) << 8),
                        );
                        self.phase = 20045;
                        return Ok(event);
                    }
                }
                if *remaining == 0 {
                    return Err(Error::new(pc, "麻将牌山耗尽结算尚未接入"));
                }
                self.chi_choices.clear();
                self.seat = next as u32;
                event = 20046;
                packet[0] = event;
                20013
            }
            20013 => {
                packet.push(self.seat);
                20014
            }
            20014 => {
                let ordinal = self.next_wall_ordinal();
                self.draw(ordinal, packet);
                *remaining -= 1;
                20019
            }
            20040 => {
                packet.push(((seat + 1) % seats) as u32);
                20041
            }
            20041 => {
                self.apply_chi(seats, packet);
                20007
            }
            20042 => 20043,
            20043 => 20043,
            _ => return Err(Error::new(pc, format!("麻将牌局事件 {event} 尚未接入"))),
        };
        Ok(event)
    }

    fn has_other_claim(&self, seat: usize, seats: usize, remaining: u32, discard: usize) -> bool {
        (0..seats).filter(|&other| other != seat).any(|other| {
            let mut counts = [0; 34];
            for &tile in &self.hands[other] {
                counts[tile_kind(tile)] += 1;
            }
            counts[discard] += 1;
            complete(&counts) || remaining != 0 && counts[discard] >= 3
        })
    }

    fn apply_chi(&mut self, seats: usize, packet: &mut Vec<u32>) {
        let previous = self.seat as usize;
        let next = (previous + 1) % seats;
        let [i, j] = self.selected_chi.unwrap();
        let discards = &mut self.discards[previous];
        let discard_index = discards.len() - 1;
        let discard = &mut discards[discard_index];
        let hand = &mut self.hands[next];
        let mut moves = [
            (*discard, REVEALED | (self.seat << 8) | discard_index as u32),
            (hand[i], 0x2_0000 | ((next as u32) << 8) | i as u32),
            (hand[j], 0x2_0000 | ((next as u32) << 8) | j as u32),
        ];
        if seats == 2 {
            moves.swap(0, 1);
        }
        for (slot, (tile, source)) in moves.into_iter().enumerate() {
            packet.extend([tile, source, 0x3_0000 | ((next as u32) << 8) | slot as u32]);
        }
        *discard |= CLAIMED;
        hand[i] |= TAKEN;
        hand[j] |= TAKEN;
        self.seat = next as u32;
        self.meld_count[next] += 1;
        self.chi_choices.clear();
    }

    fn needs_special_action(&self, seat: usize, can_quad: bool, can_declare_ready: bool) -> bool {
        let hand = &self.hands[seat];
        let mut counts = [0; 34];
        for &tile in hand {
            counts[tile_kind(tile)] += 1;
        }
        if can_quad && counts.contains(&4) || complete(&counts) {
            return true;
        }
        if can_declare_ready {
            for original in 0..34 {
                if counts[original] == 0 {
                    continue;
                }
                counts[original] -= 1;
                for added in 0..34 {
                    if counts[added] == 4 {
                        continue;
                    }
                    counts[added] += 1;
                    if complete(&counts) {
                        return true;
                    }
                    counts[added] -= 1;
                }
                counts[original] += 1;
            }
        }
        false
    }
}

fn chi_choices(hand: &[u32], discard: u32) -> Vec<[usize; 2]> {
    let mut choices = Vec::new();
    let mut seen = Vec::new();
    let discard = tile_kind(discard);
    if discard >= 27 {
        return choices;
    }
    for (i, &first) in hand.iter().enumerate() {
        for (j, &second) in hand.iter().enumerate().skip(i + 1) {
            let mut kinds = [tile_kind(first), tile_kind(second), discard];
            kinds.sort_unstable();
            if kinds[0] / 9 != discard / 9
                || kinds[2] / 9 != discard / 9
                || kinds[1] != kinds[0] + 1
                || kinds[2] != kinds[1] + 1
            {
                continue;
            }
            let key = [first & 0x13f0, second & 0x13f0];
            if !seen.contains(&key) {
                seen.push(key);
                choices.push([i, j]);
            }
        }
    }
    choices
}

fn tile_kind(tile: u32) -> usize {
    ((tile >> 4 & 15) + (tile >> 8 & 3) * 9) as usize
}

// Distinguish hands that may need a special action from ordinary discards. Scoring is separate.
fn complete(counts: &[u8; 34]) -> bool {
    const TERMINALS: [usize; 13] = [0, 8, 9, 17, 18, 26, 27, 28, 29, 30, 31, 32, 33];
    if counts.iter().filter(|&&count| count == 2).count() == 7
        || TERMINALS.iter().all(|&kind| counts[kind] != 0)
    {
        return true;
    }
    let mut rest = *counts;
    for kind in 0..34 {
        if rest[kind] >= 2 {
            rest[kind] -= 2;
            if melds(&mut rest) {
                return true;
            }
            rest[kind] += 2;
        }
    }
    false
}

fn melds(counts: &mut [u8; 34]) -> bool {
    let Some(first) = counts.iter().position(|&count| count != 0) else {
        return true;
    };
    if counts[first] >= 3 {
        counts[first] -= 3;
        let fits = melds(counts);
        counts[first] += 3;
        if fits {
            return true;
        }
    }
    if first < 27 && first % 9 < 7 && counts[first + 1] != 0 && counts[first + 2] != 0 {
        for value in &mut counts[first..first + 3] {
            *value -= 1;
        }
        let fits = melds(counts);
        for value in &mut counts[first..first + 3] {
            *value += 1;
        }
        return fits;
    }
    false
}
