#[derive(Clone, Debug, Default)]
pub struct Shelf {
    pub y_offset: u32,
    pub height: u32,
    pub x_cursor: u32,
}

#[derive(Clone, Debug)]
pub struct ShelfPacker {
    pub shelves: Vec<Shelf>,
    pub atlas_width: u32,
    pub atlas_height: u32,
}

impl ShelfPacker {
    pub fn new(atlas_width: u32, atlas_height: u32) -> Self {
        Self {
            shelves: Vec::new(),
            atlas_width,
            atlas_height,
        }
    }

    pub fn allocate(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        if width == 0 || height == 0 || width > self.atlas_width || height > self.atlas_height {
            return None;
        }

        for shelf in &mut self.shelves {
            if height <= shelf.height && shelf.x_cursor + width <= self.atlas_width {
                let origin = (shelf.x_cursor, shelf.y_offset);
                shelf.x_cursor += width;
                return Some(origin);
            }
        }

        let y_offset = self
            .shelves
            .last()
            .map(|shelf| shelf.y_offset + shelf.height)
            .unwrap_or(0);
        if y_offset + height > self.atlas_height {
            return None;
        }

        self.shelves.push(Shelf {
            y_offset,
            height,
            x_cursor: width,
        });
        Some((0, y_offset))
    }
}
