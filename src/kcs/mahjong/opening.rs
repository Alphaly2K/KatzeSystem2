use super::Mahjong;
use crate::{Error, Result, kcs::random::Stream};

mod turns;

const WALL_SIZE: usize = 136;
const SIDE_SIZE: usize = WALL_SIZE / 4;
const TAKEN: u32 = 0x8000_0000;

pub(super) struct Opening {
    pub(super) phase: u32,
    pub(super) seat: u32,
    wall: [u32; WALL_SIZE],
    wall_ready: bool,
    draw_order: [usize; WALL_SIZE],
    next_draw: usize,
    hands: [Vec<u32>; 4],
    discards: [Vec<u32>; 4],
    discard_index: usize,
    random: Stream,
}

impl Default for Opening {
    fn default() -> Self {
        Self {
            phase: 0,
            seat: 0,
            wall: [0; WALL_SIZE],
            wall_ready: false,
            draw_order: [0; WALL_SIZE],
            next_draw: 0,
            hands: std::array::from_fn(|_| Vec::new()),
            discards: std::array::from_fn(|_| Vec::new()),
            discard_index: 0,
            random: Stream::seeded_words(4357),
        }
    }
}

impl Opening {
    pub(super) fn seed(&mut self, seed: u32) {
        self.random = Stream::seeded_words(seed);
    }

    pub(super) fn wall_side(&self, side: usize) -> &[u32] {
        if !self.wall_ready {
            return &[];
        }
        &self.wall[side * SIDE_SIZE..(side + 1) * SIDE_SIZE]
    }

    pub(super) fn hand(&self, seat: usize) -> &[u32] {
        &self.hands[seat]
    }

    fn shuffle(&mut self, red: [u32; 3]) {
        let mut order: [usize; WALL_SIZE] = std::array::from_fn(|tile| tile);
        for index in 0..WALL_SIZE {
            let other = self.random.next() as usize % WALL_SIZE;
            order.swap(index, other);
        }
        for (word, tile) in self.wall.iter_mut().zip(order) {
            let (suit, rank, copy) = (tile / 36, tile / 4 % 9, tile % 4);
            *word = ((suit << 8) | (rank << 4) | copy) as u32;
            if suit < 3 && rank == 4 && (copy as i32) < red[suit] as i32 {
                *word |= 0x1000;
            }
        }
        self.wall_ready = true;
        self.hands.iter_mut().for_each(Vec::clear);
        self.discards.iter_mut().for_each(Vec::clear);
    }

    pub(super) fn roll(&mut self, dealer: u32, first: u32, second: u32) {
        if self.phase != 10004 || !(1..=6).contains(&first) || !(1..=6).contains(&second) {
            return;
        }
        let sum = (first + second) as usize;
        let mut side = (dealer as usize).wrapping_add(sum).wrapping_sub(1) & 3;
        let mut column = SIDE_SIZE - 1 - sum * 2;
        for pair in self.draw_order.chunks_exact_mut(2) {
            pair[0] = side * SIDE_SIZE + column;
            pair[1] = pair[0] - 1;
            if column == 1 {
                side = side.wrapping_sub(1) & 3;
                column = SIDE_SIZE - 1;
            } else {
                column -= 2;
            }
        }
        self.next_draw = 0;
        self.phase = 20001;
    }

    fn randomize_prefix<T>(&mut self, values: &mut [T], count: usize) {
        for i in 0..count.min(values.len()) {
            let other = usize::from(self.random.next() as u16) % values.len();
            values.swap(i, other);
        }
    }

    fn select_groups(
        &mut self,
        types: &mut [Vec<usize>; 64],
        positions: &mut Vec<usize>,
        size: usize,
        mut count: usize,
    ) {
        for size in (1..=size).rev() {
            if count == 0 {
                break;
            }
            let mut eligible: Vec<_> = (0..types.len())
                .filter(|&kind| types[kind].len() >= size)
                .collect();
            self.randomize_prefix(&mut eligible, count);
            let selected = count.min(eligible.len());
            for kind in eligible.into_iter().take(selected) {
                let available = &mut types[kind];
                for _ in 0..size {
                    let index = usize::from(self.random.next() as u16) % available.len();
                    positions.push(available.swap_remove(index));
                }
            }
            count -= selected;
        }
    }

    fn arrange_default_seat(&mut self, seats: usize, relative_seat: usize) {
        let mut types: [Vec<usize>; 64] = std::array::from_fn(|_| Vec::new());
        let mut suits = [0; 4];
        for index in seats * 13..122 {
            let kind = (self.wall[self.draw_order[index]] >> 4 & 63) as usize;
            types[kind].push(index);
            suits[kind / 16] += 1;
        }
        let mut largest = usize::from(suits[0] < suits[1]);
        if suits[largest] < suits[2] {
            largest = 2;
        }
        for (kind, group) in types.iter_mut().enumerate() {
            if kind / 16 != largest {
                group.clear();
            }
        }
        let mut selected = Vec::new();
        for count in [2, 2, 1] {
            self.select_groups(&mut types, &mut selected, 1, count);
        }

        for index in seats * 13..122 {
            let kind = (self.wall[self.draw_order[index]] >> 4 & 63) as usize;
            if kind / 16 == 3 {
                types[kind].push(index);
            }
        }
        types[..48].iter_mut().for_each(Vec::clear);
        self.select_groups(&mut types, &mut selected, 2, 2);

        let mut destinations: [usize; 13] = std::array::from_fn(|slot| {
            if slot < 12 {
                (slot & !3) * seats + relative_seat * 4 + (slot & 3)
            } else {
                seats * 12 + relative_seat
            }
        });
        self.randomize_prefix(&mut destinations, 13);
        for (source, destination) in selected.into_iter().zip(destinations) {
            self.wall
                .swap(self.draw_order[source], self.draw_order[destination]);
        }
    }

    fn deal(&mut self, seats: usize, dealer: u32, packet: &mut Vec<u32>) -> bool {
        let seat = self.seat as usize;
        let hand_len = self.hands[seat].len();
        let count = if hand_len < 12 {
            4
        } else if self.seat == dealer {
            2
        } else {
            1
        };
        for draw in 0..count {
            // The dealer's extra tile is taken a full seat ahead of the first tile.
            let ordinal = if (draw | count) == 3 {
                (self.next_draw + seats - 1) % WALL_SIZE
            } else {
                while self.wall[self.draw_order[self.next_draw]] & TAKEN != 0 {
                    self.next_draw = (self.next_draw + 1) % WALL_SIZE;
                }
                let ordinal = self.next_draw;
                self.next_draw = (self.next_draw + 1) % WALL_SIZE;
                ordinal
            };
            let location = self.draw_order[ordinal];
            let tile = self.wall[location];
            self.wall[location] |= TAKEN;
            let side = location / SIDE_SIZE;
            let column = location % SIDE_SIZE;
            packet.extend([
                tile,
                ((side << 8) | column) as u32,
                0x2_0000 | (self.seat << 8) | (hand_len + draw) as u32,
            ]);
            self.hands[seat].push(tile);
        }
        self.seat = ((seat + 1) % seats) as u32;
        count < 4 && self.seat == dealer
    }
}

impl Mahjong {
    pub(super) fn poll(&mut self, handle: u32, pc: usize) -> Result<u32> {
        let opening = &mut self.opening;
        let mut phase = opening.phase;
        let dealer = self.settings.first_dealer;
        let mut packet = vec![phase];
        match phase {
            0 | 10004 => {}
            10003 => {
                opening.shuffle(self.settings.round_options);
                self.remaining = if self.seats == 2 { 61 } else { 122 };
                opening.phase = 10004;
            }
            20001 => opening.phase = 20002,
            20002 => {
                const PRESETS: [u32; 20] =
                    [0, 3, 1, 3, 1, 1, 7, 0, 1, 3, 0, 1, 3, 3, 39, 3, 3, 3, 7, 3];
                let preset = |value: u32| PRESETS.get(value as usize).copied().unwrap_or(0);
                let options = &self.settings.seat_options[1..self.seats];
                if options
                    .iter()
                    .any(|&value| !matches!(preset(value), 0 | 3 | 39))
                {
                    return Err(Error::new(pc, "麻将座位牌型预设尚未接入"));
                }
                if dealer as usize >= self.seats {
                    return Err(Error::new(pc, "麻将庄家座位越界"));
                }
                for (seat, &value) in options.iter().enumerate() {
                    if preset(value) == 0 {
                        let relative = (self.seats + seat + 1 - dealer as usize) % self.seats;
                        opening.arrange_default_seat(self.seats, relative);
                    }
                }
                opening.phase = 20003;
            }
            20003 => {
                opening.seat = dealer;
                opening.phase = 20004;
            }
            20004 => {
                if opening.deal(self.seats, dealer, &mut packet) {
                    opening.phase = 20005;
                }
                self.remaining -= ((packet.len() - 1) / 3) as u32;
            }
            20005 => opening.phase = 20006,
            20006 => {
                for seat in 0..self.seats {
                    opening.hands[seat].sort_by_key(|tile| tile & 0x8000_03ff);
                    packet.push(seat as u32);
                }
                opening.phase = 20009;
            }
            _ => {
                phase =
                    opening.poll_turn(&mut packet, self.points, self.remaining, self.seats, pc)?;
            }
        }
        *self.handles.get_mut(&handle).unwrap() = packet;
        Ok(phase)
    }
}
