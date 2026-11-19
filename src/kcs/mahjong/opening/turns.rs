use super::{Opening, SIDE_SIZE, TAKEN};
use crate::{Error, Result};

const REVEALED: u32 = 0x10000;

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

    pub(super) fn poll_turn(
        &mut self,
        packet: &mut Vec<u32>,
        points: [u32; 4],
        remaining: u32,
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
                20021
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
                let can_declare_ready =
                    (points[seat] as i32) >= 1000 && remaining as usize >= seats;
                if self.needs_special_action(seat, remaining != 0, can_declare_ready) {
                    return Err(Error::new(pc, "麻将出牌前的特殊选项尚未接入"));
                }
                event = 20042;
                packet[0] = event;
                20043
            }
            20043 => 20043,
            _ => return Err(Error::new(pc, format!("麻将牌局事件 {event} 尚未接入"))),
        };
        Ok(event)
    }

    fn needs_special_action(&self, seat: usize, can_quad: bool, can_declare_ready: bool) -> bool {
        let hand = &self.hands[seat];
        let mut counts = [0; 34];
        for &tile in hand {
            let kind = (tile >> 4 & 15) + (tile >> 8 & 3) * 9;
            counts[kind as usize] += 1;
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
