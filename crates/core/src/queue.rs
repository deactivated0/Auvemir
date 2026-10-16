use crate::model::Media;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}
#[derive(Default, Debug)]
pub struct Queue {
    pub items: Vec<Media>,
    pub cursor: usize,
    pub repeat: Repeat,
}
impl Queue {
    pub fn current(&self) -> Option<&Media> {
        self.items.get(self.cursor)
    }
    pub fn replace(&mut self, items: Vec<Media>, cursor: usize) {
        self.cursor = cursor.min(items.len().saturating_sub(1));
        self.items = items;
    }
    pub fn advance(&mut self, automatic: bool) -> bool {
        if self.items.is_empty() {
            return false;
        }
        if automatic && self.repeat == Repeat::One {
            return true;
        }
        if self.cursor + 1 < self.items.len() {
            self.cursor += 1;
            true
        } else if self.repeat == Repeat::All {
            self.cursor = 0;
            true
        } else {
            false
        }
    }
    pub fn previous(&mut self) -> bool {
        if self.items.is_empty() {
            false
        } else {
            self.cursor = self.cursor.saturating_sub(1);
            true
        }
    }
    pub fn remove(&mut self, index: usize) {
        if index >= self.items.len() {
            return;
        }
        self.items.remove(index);
        if index < self.cursor {
            self.cursor -= 1;
        }
        self.cursor = self.cursor.min(self.items.len().saturating_sub(1));
    }
    /// Insert immediately after the active entry without interrupting playback.
    pub fn insert_next(&mut self, media: Media) {
        let index = if self.items.is_empty() {
            0
        } else {
            (self.cursor + 1).min(self.items.len())
        };
        self.items.insert(index, media);
    }
    /// Move an entry while preserving the identity of the active entry, including duplicates.
    pub fn move_item(&mut self, from: usize, to: usize) -> bool {
        if from >= self.items.len() || to >= self.items.len() || from == to {
            return false;
        }
        let item = self.items.remove(from);
        self.items.insert(to, item);
        if self.cursor == from {
            self.cursor = to;
        } else if from < self.cursor && to >= self.cursor {
            self.cursor -= 1;
        } else if from > self.cursor && to <= self.cursor {
            self.cursor += 1;
        }
        true
    }
    pub fn shuffle(&mut self, mut seed: u64) {
        // Preserve the active item and already-played prefix.
        if seed == 0 {
            seed = 0x9e3779b97f4a7c15;
        }
        let start = if self.items.is_empty() {
            0
        } else {
            self.cursor + 1
        };
        for i in (start + 1..self.items.len()).rev() {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let j = start + (seed as usize % (i - start + 1));
            self.items.swap(i, j);
        }
    }
    pub fn cycle_repeat(&mut self) {
        self.repeat = match self.repeat {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        };
    }
}
