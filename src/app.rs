use crate::api::Api;
use crate::bottom_panel::BottomPanel;
use crate::spawner::Spawner;
use crate::top_panel::TopPanel;
use crate::wm::Wm;
use std::cell::OnceCell;
use std::rc::Rc;

pub struct App {
    api: Api,
    wm: OnceCell<Wm>,
    top_panel: OnceCell<TopPanel>,
    bottom_panel: OnceCell<BottomPanel>,
    spawner: OnceCell<Spawner>,
}

impl App {
    pub fn new() -> Rc<Self> {
        let this = Rc::new(Self {
            api: Api::new(),
            wm: OnceCell::new(),
            top_panel: OnceCell::new(),
            bottom_panel: OnceCell::new(),
            spawner: OnceCell::new(),
        });

        let _ = this.wm.set(Wm::new(this.clone()));
        let _ = this.top_panel.set(TopPanel::new(this.clone()));
        let _ = this.bottom_panel.set(BottomPanel::new(this.clone()));
        let _ = this.spawner.set(Spawner::new());

        this
    }

    pub fn api(&self) -> &Api {
        &self.api
    }

    pub fn wm(&self) -> &Wm {
        self.wm.get().unwrap()
    }

    pub fn top_panel(&self) -> &TopPanel {
        self.top_panel.get().unwrap()
    }

    pub fn bottom_panel(&self) -> &BottomPanel {
        self.bottom_panel.get().unwrap()
    }

    pub fn spawner(&self) -> &Spawner {
        self.spawner.get().unwrap()
    }
}
