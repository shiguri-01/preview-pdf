use crate::render::cache::RenderedPageKey;
use crate::render::prefetch::{PrefetchQueue, QueueTaskMeta};
use crate::work::WorkClass;

const MAX_PREFETCH_DEPTH: usize = 3;
const GUARD_REVERSE_DEPTH: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavDirection {
    Forward,
    Backward,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavIntent {
    pub dir: NavDirection,
    pub streak: usize,
    pub generation: u64,
}

impl Default for NavIntent {
    fn default() -> Self {
        Self {
            dir: NavDirection::Forward,
            streak: 0,
            generation: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderTask {
    pub doc_id: u64,
    pub page: usize,
    pub scale: f32,
    pub class: WorkClass,
    pub generation: u64,
}

#[derive(Debug)]
pub struct RenderScheduler {
    tasks: PrefetchQueue<RenderedPageKey, RenderTask>,
}

impl Default for RenderScheduler {
    fn default() -> Self {
        Self {
            tasks: PrefetchQueue::new(),
        }
    }
}

impl RenderScheduler {
    pub fn enqueue(&mut self, task: RenderTask) {
        let key = RenderedPageKey::new(task.doc_id, task.page, task.scale);
        let meta = QueueTaskMeta {
            key,
            class: task.class,
            generation: task.generation,
        };
        let _ = self.tasks.push(task, meta);
    }

    pub fn next_task(&mut self) -> Option<RenderTask> {
        self.tasks.pop_next()
    }

    pub fn cancel_obsolete(&mut self, nav_intent: NavIntent, scale: f32) -> usize {
        self.tasks
            .retain(|task, _| !should_cancel(task, nav_intent, scale))
    }

    pub fn cancel_stale_prefetch(&mut self, generation: u64) -> usize {
        self.tasks.cancel_stale_prefetch(generation)
    }

    pub fn clear(&mut self) -> usize {
        self.tasks.clear()
    }

    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }
}

pub fn build_prefetch_plan(
    cursor: usize,
    nav_intent: NavIntent,
    page_count: usize,
    doc_id: u64,
    scale: f32,
) -> Vec<RenderTask> {
    if page_count == 0 {
        return Vec::new();
    }

    let mut tasks = Vec::new();
    let depth = dynamic_depth(nav_intent.streak).min(MAX_PREFETCH_DEPTH);
    let cursor = cursor.min(page_count - 1);

    tasks.push(RenderTask {
        doc_id,
        page: cursor,
        scale,
        class: WorkClass::CriticalCurrent,
        generation: nav_intent.generation,
    });

    match nav_intent.dir {
        NavDirection::Forward => {
            push_relative(
                &mut tasks,
                cursor,
                1,
                page_count,
                doc_id,
                scale,
                WorkClass::DirectionalLead,
                nav_intent.generation,
            );

            for i in 1..=GUARD_REVERSE_DEPTH {
                push_relative(
                    &mut tasks,
                    cursor,
                    -(i as isize),
                    page_count,
                    doc_id,
                    scale,
                    WorkClass::GuardReverse,
                    nav_intent.generation,
                );
            }

            for i in 2..=depth {
                push_relative(
                    &mut tasks,
                    cursor,
                    i as isize,
                    page_count,
                    doc_id,
                    scale,
                    WorkClass::DirectionalLead,
                    nav_intent.generation,
                );
            }

            if depth >= 3 {
                push_relative(
                    &mut tasks,
                    cursor,
                    -((GUARD_REVERSE_DEPTH + 1) as isize),
                    page_count,
                    doc_id,
                    scale,
                    WorkClass::Background,
                    nav_intent.generation,
                );
            }
        }
        NavDirection::Backward => {
            push_relative(
                &mut tasks,
                cursor,
                -1,
                page_count,
                doc_id,
                scale,
                WorkClass::DirectionalLead,
                nav_intent.generation,
            );

            for i in 1..=GUARD_REVERSE_DEPTH {
                push_relative(
                    &mut tasks,
                    cursor,
                    i as isize,
                    page_count,
                    doc_id,
                    scale,
                    WorkClass::GuardReverse,
                    nav_intent.generation,
                );
            }

            for i in 2..=depth {
                push_relative(
                    &mut tasks,
                    cursor,
                    -(i as isize),
                    page_count,
                    doc_id,
                    scale,
                    WorkClass::DirectionalLead,
                    nav_intent.generation,
                );
            }

            if depth >= 3 {
                push_relative(
                    &mut tasks,
                    cursor,
                    (GUARD_REVERSE_DEPTH + 1) as isize,
                    page_count,
                    doc_id,
                    scale,
                    WorkClass::Background,
                    nav_intent.generation,
                );
            }
        }
    }

    tasks
}

pub fn should_cancel(task: &RenderTask, nav_intent: NavIntent, scale: f32) -> bool {
    let scale_changed = ((task.scale * 1000.0).round() as i64) != ((scale * 1000.0).round() as i64);
    if scale_changed {
        return true;
    }

    if task.generation >= nav_intent.generation {
        return false;
    }

    if nav_intent.streak == 0 {
        return true;
    }

    matches!(
        task.class,
        WorkClass::DirectionalLead | WorkClass::Background
    )
}

fn dynamic_depth(streak: usize) -> usize {
    match streak {
        0 | 1 => 1,
        2..=4 => 2,
        _ => 3,
    }
}

#[allow(clippy::too_many_arguments)]
fn push_relative(
    out: &mut Vec<RenderTask>,
    cursor: usize,
    offset: isize,
    page_count: usize,
    doc_id: u64,
    scale: f32,
    class: WorkClass,
    generation: u64,
) {
    let pos = cursor as isize + offset;
    if pos < 0 || pos >= page_count as isize {
        return;
    }
    out.push(RenderTask {
        doc_id,
        page: pos as usize,
        scale,
        class,
        generation,
    });
}

#[cfg(test)]
mod tests {
    use super::{
        NavDirection, NavIntent, RenderScheduler, RenderTask, build_prefetch_plan as build_plan,
        should_cancel,
    };
    use crate::work::WorkClass;

    fn build_prefetch_plan(
        cursor: usize,
        nav_intent: NavIntent,
        page_count: usize,
    ) -> Vec<RenderTask> {
        build_plan(cursor, nav_intent, page_count, 0, 1.0)
    }

    #[test]
    fn prefetch_forward_order_matches_rule() {
        let intent = NavIntent {
            dir: NavDirection::Forward,
            streak: 9,
            generation: 2,
        };
        let tasks = build_prefetch_plan(10, intent, 40);
        let pages: Vec<usize> = tasks.iter().map(|t| t.page).collect();

        assert_eq!(pages, vec![10, 11, 9, 12, 13, 8]);
        assert_eq!(tasks[0].class, WorkClass::CriticalCurrent);
        assert_eq!(tasks[2].class, WorkClass::GuardReverse);
        assert_eq!(tasks[5].class, WorkClass::Background);
    }

    #[test]
    fn prefetch_depth_changes_with_streak() {
        let shallow = build_prefetch_plan(
            5,
            NavIntent {
                dir: NavDirection::Forward,
                streak: 1,
                generation: 0,
            },
            20,
        );
        let medium = build_prefetch_plan(
            5,
            NavIntent {
                dir: NavDirection::Forward,
                streak: 3,
                generation: 0,
            },
            20,
        );

        assert_eq!(shallow.len(), 3);
        assert_eq!(medium.len(), 4);
    }

    #[test]
    fn scheduler_pops_highest_priority_first() {
        let mut scheduler = RenderScheduler::default();
        scheduler.enqueue(RenderTask {
            doc_id: 1,
            page: 2,
            scale: 1.0,
            class: WorkClass::Background,
            generation: 1,
        });
        scheduler.enqueue(RenderTask {
            doc_id: 1,
            page: 1,
            scale: 1.0,
            class: WorkClass::CriticalCurrent,
            generation: 1,
        });

        let first = scheduler.next_task().expect("task should exist");
        assert_eq!(first.class, WorkClass::CriticalCurrent);
    }

    #[test]
    fn should_cancel_respects_generation_and_scale() {
        let task = RenderTask {
            doc_id: 3,
            page: 9,
            scale: 1.0,
            class: WorkClass::DirectionalLead,
            generation: 1,
        };

        let nav = NavIntent {
            dir: NavDirection::Backward,
            streak: 2,
            generation: 2,
        };
        assert!(should_cancel(&task, nav, 1.0));
        assert!(should_cancel(&task, nav, 1.5));
    }

    #[test]
    fn cancel_obsolete_counts_removed_tasks() {
        let mut scheduler = RenderScheduler::default();
        scheduler.enqueue(RenderTask {
            doc_id: 1,
            page: 5,
            scale: 1.0,
            class: WorkClass::DirectionalLead,
            generation: 1,
        });
        scheduler.enqueue(RenderTask {
            doc_id: 1,
            page: 4,
            scale: 1.0,
            class: WorkClass::GuardReverse,
            generation: 1,
        });

        let canceled = scheduler.cancel_obsolete(
            NavIntent {
                dir: NavDirection::Backward,
                streak: 2,
                generation: 2,
            },
            1.0,
        );
        assert_eq!(canceled, 1);
    }
}
