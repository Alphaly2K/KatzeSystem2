pub(super) struct SystemInput {
    pub raw: u32,
    pub left: bool,
    pub mode: bool,
    pub cancel_enabled: bool,
    pub ctrl: bool,
    pub continuous: bool,
    pub forced: bool,
    transient: bool,
}

impl Default for SystemInput {
    fn default() -> Self {
        Self {
            raw: 0,
            left: false,
            mode: false,
            cancel_enabled: true,
            ctrl: false,
            continuous: false,
            forced: false,
            transient: false,
        }
    }
}

pub(super) const TRANSIENT_NAMES: [&str; 11] = [
    "_CLICK_L_",
    "_CLICK_R_",
    "_SCRL_U_",
    "_SCRL_D_",
    "_SCRL_P_",
    "_PAGE_U_",
    "_PAGE_D_",
    "_PUSH_UP_",
    "_PUSH_DOWN_",
    "_PUSH_RIGHT_",
    "_PUSH_LEFT_",
];

impl SystemInput {
    pub fn record(&mut self, event: u32) -> Option<(&'static str, i32)> {
        let name = match event {
            2 | 19 => {
                self.left = true;
                "_CLICK_L_"
            }
            3 => "_CLICK_R_",
            20 => {
                self.left = false;
                "_CLICK_L_"
            }
            4 => "_CLICK_L_",
            5 => "_CLICK_R_",
            6 => "_SCRL_U_",
            7 => "_SCRL_D_",
            8 => "_SCRL_P_",
            9 => "_PAGE_U_",
            10 => "_PAGE_D_",
            11 => "_PUSH_UP_",
            12 => "_PUSH_DOWN_",
            13 => "_PUSH_LEFT_",
            14 => "_PUSH_RIGHT_",
            15 => "_RELEASE_UP_",
            16 => "_RELEASE_DOWN_",
            17 => "_RELEASE_LEFT_",
            18 => "_RELEASE_RIGHT_",
            21 => "_TOUCH_",
            _ => return None,
        };
        if matches!(event, 2 | 3 | 6..=14 | 21) {
            self.transient = true;
        }
        if matches!(event, 19 | 20) {
            self.mode = event == 19;
        }
        Some((name, if matches!(event, 4 | 5 | 20) { 0 } else { -1 }))
    }

    pub fn finish(&mut self) -> bool {
        if !self.mode && !self.continuous {
            self.left = false;
        }
        std::mem::take(&mut self.transient)
    }
}
