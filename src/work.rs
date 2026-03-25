#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkClass {
    CriticalCurrent,
    GuardReverse,
    DirectionalLead,
    Background,
}

impl WorkClass {
    pub(crate) const fn rank(self) -> u8 {
        match self {
            Self::CriticalCurrent => 4,
            Self::GuardReverse => 3,
            Self::DirectionalLead => 2,
            Self::Background => 1,
        }
    }

    pub(crate) const fn is_prefetch(self) -> bool {
        matches!(self, Self::DirectionalLead | Self::Background)
    }

    pub(crate) const fn kept_on_background_stale_generation(self) -> bool {
        matches!(self, Self::CriticalCurrent | Self::GuardReverse)
    }

    pub(crate) const fn preempt_rank(
        self,
        current_generation: u64,
        task_generation: u64,
    ) -> Option<(u8, u8, u64)> {
        let class_rank = match self {
            Self::Background => 0,
            Self::DirectionalLead => 1,
            _ => return None,
        };
        let stale_rank = if task_generation < current_generation {
            0
        } else {
            1
        };
        Some((stale_rank, class_rank, task_generation))
    }
}
