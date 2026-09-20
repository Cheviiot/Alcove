// SPDX-License-Identifier: GPL-3.0-only
//! CEF's in-page popup surface. It shares the site's coordinate/input space,
//! but never contributes to the page's measured size or accessibility tree.
use anyhow::{ensure, Result};
use gtk::{gdk, glib, prelude::*, subclass::prelude::*};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

mod imp {
    use super::*;
    #[derive(Default)]
    pub struct Layer {
        pub frame: std::cell::RefCell<Option<(gdk::Texture, [i32; 4])>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Layer {
        const NAME: &'static str = "AlcoveProbePopupSurface";
        type Type = super::Layer;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for Layer {}
    impl WidgetImpl for Layer {
        fn measure(&self, _: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            (0, 0, -1, -1)
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            if let Some((texture, [x, y, w, h])) = self.frame.borrow().as_ref() {
                // CEF supplies physical pixels but bounds are logical. Drawing
                // explicitly avoids the texture's intrinsic pixel dimensions
                // changing the layout of a HiDPI popup.
                snapshot.append_texture(
                    texture,
                    &gtk::graphene::Rect::new(*x as f32, *y as f32, *w as f32, *h as f32),
                );
            }
        }
    }
}
glib::wrapper! {
    pub struct Layer(ObjectSubclass<imp::Layer>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

#[derive(Default, Deserialize)]
struct State {
    generation: u64,
    visible: bool,
    rect: [i32; 4],
}
impl State {
    fn accepts(&self, generation: u64) -> bool {
        self.visible && self.generation == generation && self.rect[2] > 0 && self.rect[3] > 0
    }
    fn update(&mut self, next: State) -> Result<bool> {
        let limit = super::protocol::MAX_DIMENSION as i32;
        ensure!(
            next.generation > 0
                && (-limit..=limit).contains(&next.rect[0])
                && (-limit..=limit).contains(&next.rect[1])
                && (0..=limit).contains(&next.rect[2])
                && (0..=limit).contains(&next.rect[3]),
            "invalid popup surface state"
        );
        if next.generation <= self.generation {
            return Ok(false);
        }
        *self = next;
        Ok(true)
    }
}

pub struct PopupSurface {
    pub layer: Layer,
    state: State,
    gpu: Option<super::gpu::Texture>,
    cpu: Option<u64>,
    painted_generation: u64,
    presented: u64,
    observations: Vec<Value>,
}
impl PopupSurface {
    pub fn new() -> Self {
        let layer = glib::Object::builder::<Layer>()
            .property("accessible-role", gtk::AccessibleRole::Presentation)
            .property("can-target", false)
            .property("overflow", gtk::Overflow::Hidden)
            .property("visible", false)
            .build();
        Self {
            layer,
            state: State::default(),
            gpu: None,
            cpu: None,
            painted_generation: 0,
            presented: 0,
            observations: Vec::new(),
        }
    }
    pub fn update(&mut self, event: &Value) -> Result<()> {
        if self.state.update(serde_json::from_value(event.clone())?)? {
            self.layer.set_visible(false);
            self.layer.imp().frame.take();
            if self
                .gpu
                .as_ref()
                .is_some_and(|frame| frame.generation < self.state.generation)
            {
                self.gpu = None;
            }
            if self
                .cpu
                .is_some_and(|generation| generation < self.state.generation)
            {
                self.cpu = None;
            }
            if self.observations.len() < 256 {
                self.observations
                    .push(json!({"generation":self.state.generation,
                    "visible":self.state.visible,"rect":self.state.rect}));
            }
        }
        Ok(())
    }
    pub fn queue_gpu(&mut self, frame: super::gpu::Texture) {
        if frame.generation >= self.state.generation {
            self.gpu = Some(frame);
        }
    }
    pub fn queue_cpu(&mut self, generation: u64) {
        if generation >= self.state.generation {
            self.cpu = Some(generation);
        }
    }
    /// Pipes and the GPU socket can be observed in different orders. Keep one
    /// future frame until its state arrives; never display a retired generation.
    pub fn present(&mut self, directory: &Path, scale: i32) -> Result<Option<u64>> {
        let texture = if self
            .gpu
            .as_ref()
            .is_some_and(|frame| self.state.accepts(frame.generation))
        {
            self.gpu.take().map(|frame| frame.texture)
        } else if self
            .cpu
            .is_some_and(|generation| self.state.accepts(generation))
        {
            let generation = self.cpu.take().unwrap();
            let path = directory.join(format!("popup-{generation}.bin"));
            match super::cpu_texture(&path) {
                Ok(texture) => Some(texture),
                // CEF can close the popup and remove its file before the pipe
                // reader observes the hide event. No stale image is substituted.
                Err(error)
                    if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
                {
                    None
                }
                Err(error) => return Err(error),
            }
        } else {
            None
        };
        let Some(texture) = texture else {
            return Ok(None);
        };
        ensure!(
            texture.width() == self.state.rect[2] * scale
                && texture.height() == self.state.rect[3] * scale,
            "popup texture does not match logical bounds and scale"
        );
        self.layer
            .imp()
            .frame
            .replace(Some((texture.clone(), self.state.rect)));
        self.layer.set_visible(true);
        self.layer.queue_draw();
        self.presented += 1;
        if self.painted_generation == self.state.generation {
            return Ok(None);
        }
        self.painted_generation = self.state.generation;
        if self.observations.len() < 256 {
            self.observations.push(
                json!({"painted":self.state.generation,"rect":self.state.rect,
                "pixels":[texture.width(),texture.height()],"scale":scale}),
            );
        }
        Ok(Some(self.painted_generation))
    }
    pub fn report(&self) -> Value {
        json!({"presented":self.presented,"visible":self.layer.is_visible(),
            "generation":self.state.generation,"observations":self.observations})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delayed_popup_frames_cannot_reopen_hidden_or_replaced_surfaces() {
        let mut state = State::default();
        let next = |generation, visible| State {
            generation,
            visible,
            rect: [12, 34, 200, 100],
        };
        state.update(next(2, true)).unwrap();
        assert!(state.accepts(2));
        assert!(!state.accepts(3)); // Frame can precede its state on the other pipe.
        state.update(next(3, false)).unwrap();
        assert!(!state.accepts(2));
        assert!(!state.accepts(3));
        assert!(!state.update(next(2, true)).unwrap());
        state.update(next(4, true)).unwrap();
        assert!(!state.accepts(2));
        assert!(state.accepts(4));
    }
}
