// pathfinder/canvas/src/text_no_text.rs
//
// Copyright © 2019 The Pathfinder Project Developers.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#[derive(Clone)]
pub struct CanvasFontContext;

impl CanvasFontContext {
    pub fn from_system_source() -> Self {
        CanvasFontContext
    }
}

pub struct FontCollection;
