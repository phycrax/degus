//! The page list.

use std::collections::BTreeMap;

use degus_sim::Sim;
use degus_sim_gui::SimThread;
use degus_sim_gui::egui::{self, Button, RichText, Sense};

/// The pages to pick one from to show: those with controls, and the
/// backgrounds of the whole screen, which may be pages without controls.
#[derive(Debug, Default)]
pub(crate) struct PageList {
    /// With what is on them.
    pages: Vec<(u16, String)>,
    /// The background ICL they were listed from, until a reload.
    from: Option<u8>,
}

impl PageList {
    pub fn new() -> Self {
        Self::default()
    }

    /// The set changed: list its pages anew.
    pub fn reloaded(&mut self) {
        self.from = None;
    }

    pub fn show(&mut self, ui: &mut egui::Ui, sim: &SimThread) {
        let shown = {
            let sim = sim.lock();
            if self.from != Some(sim.background_icl()) {
                self.pages = pages(&sim);
                self.from = Some(sim.background_icl());
            }
            sim.page()
        };
        ui.heading("Pages");
        let mut show = None;
        ui.scope(|ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            egui::ScrollArea::vertical()
                .id_salt("pages")
                .show(ui, |ui| {
                    for (id, label) in &self.pages {
                        let text = RichText::new(format!("{id:>5}  {label}")).monospace();
                        // Without focus, so that Space and Enter don't pick it.
                        let item = Button::selectable(*id == shown, text).sense(Sense::CLICK);
                        if ui.add(item).clicked() {
                            show = Some(*id);
                        }
                    }
                });
        });
        if let Some(page) = show {
            sim.update(|sim| sim.show_page(page));
        }
    }
}

/// The pages to pick from. The background ICL can hold icons as well, so
/// images smaller than the screen are left out.
fn pages(sim: &Sim) -> Vec<(u16, String)> {
    let set = sim.set();
    let screen = set.device.screen;
    let mut pages: BTreeMap<u16, String> = set
        .controls
        .iter()
        .map(|(id, controls)| {
            let label = format!(
                "{} touch, {} display",
                controls.touch.len(),
                controls.display.len()
            );
            (id, label)
        })
        .collect();
    if let Some(icl) = set.icls.get(&sim.background_icl()) {
        for (&id, icon) in &icl.icons {
            if (icon.width(), icon.height()) == (screen.w, screen.h) {
                pages
                    .entry(id)
                    .or_insert_with(|| "background only".to_owned());
            }
        }
    }
    pages.into_iter().collect()
}
