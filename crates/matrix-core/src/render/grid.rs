//! Downsampled cell grid shared between the renderer, the analyzer and the TUI.

pub struct Grid {
    pub w: usize,
    pub h: usize,
    pub raw: Vec<u8>,      // pre-tone luma (analysis input)
    pub red: Vec<u8>,      // red dominance
    pub rgb: Vec<[u8; 3]>, // sampled cell colour
    pub idx: Vec<u8>,      // ramp / level index
    pub col: Vec<[u8; 3]>, // final cell colour
}

impl Grid {
    pub fn new() -> Self {
        Grid { w: 0, h: 0, raw: vec![], red: vec![], rgb: vec![], idx: vec![], col: vec![] }
    }

    pub fn ensure(&mut self, w: usize, h: usize) {
        if self.w == w && self.h == h {
            return;
        }
        let n = w * h;
        self.w = w;
        self.h = h;
        self.raw = vec![0; n];
        self.red = vec![0; n];
        self.rgb = vec![[0; 3]; n];
        self.idx = vec![0; n];
        self.col = vec![[0; 3]; n];
    }

    pub fn snapshot(&self) -> Grid {
        Grid {
            w: self.w,
            h: self.h,
            raw: self.raw.clone(),
            red: self.red.clone(),
            rgb: self.rgb.clone(),
            idx: self.idx.clone(),
            col: self.col.clone(),
        }
    }
}
