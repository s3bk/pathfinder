// pathfinder/renderer/src/gpu/perf.rs
//
// Copyright © 2020 The Pathfinder Project Developers.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Performance monitoring infrastructure.

use pathfinder_gpu::Device;
use std::mem;
use std::ops::{Add, Div};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats {
    pub path_count: usize,
    pub fill_count: usize,
    pub tile_count: usize,
    pub cpu_build_time: Duration,
    pub drawcall_count: u32,
    pub gpu_bytes_allocated: u64,
}

impl Add<RenderStats> for RenderStats {
    type Output = RenderStats;
    fn add(self, other: RenderStats) -> RenderStats {
        RenderStats {
            path_count: self.path_count + other.path_count,
            tile_count: self.tile_count + other.tile_count,
            fill_count: self.fill_count + other.fill_count,
            cpu_build_time: self.cpu_build_time + other.cpu_build_time,
            drawcall_count: self.drawcall_count + other.drawcall_count,
            gpu_bytes_allocated: self.gpu_bytes_allocated + other.gpu_bytes_allocated,
        }
    }
}

impl Div<usize> for RenderStats {
    type Output = RenderStats;
    fn div(self, divisor: usize) -> RenderStats {
        RenderStats {
            path_count: self.path_count / divisor,
            tile_count: self.tile_count / divisor,
            fill_count: self.fill_count / divisor,
            cpu_build_time: self.cpu_build_time / divisor as u32,
            drawcall_count: self.drawcall_count / divisor as u32,
            gpu_bytes_allocated: self.gpu_bytes_allocated / divisor as u64,
        }
    }
}

pub(crate) struct TimerQueryCache<D> where D: Device {
    free_queries: Vec<D::TimerQuery>,
}

pub(crate) struct PendingTimer<D> where D: Device {
    pub(crate) dice_times: Vec<TimerFuture<D>>,
    pub(crate) bin_times: Vec<TimerFuture<D>>,
    pub(crate) raster_times: Vec<TimerFuture<D>>,
    pub(crate) other_times: Vec<TimerFuture<D>>,
}

pub(crate) enum TimerFuture<D> where D: Device {
    Pending(D::TimerQuery),
    Resolved(Duration),
}

impl<D> TimerQueryCache<D> where D: Device {
    pub(crate) fn new() -> TimerQueryCache<D> {
        TimerQueryCache { free_queries: vec![] }
    }

    pub(crate) fn alloc(&mut self, device: &D) -> D::TimerQuery {
        self.free_queries.pop().unwrap_or_else(|| device.create_timer_query())
    }

    pub(crate) fn free(&mut self, old_query: D::TimerQuery) {
        self.free_queries.push(old_query);
    }
}

impl<D> PendingTimer<D> where D: Device {
    pub(crate) fn new() -> PendingTimer<D> {
        PendingTimer {
            dice_times: vec![],
            bin_times: vec![],
            raster_times: vec![],
            other_times: vec![],
        }
    }

    pub(crate) fn poll(&mut self, device: &D) -> Vec<D::TimerQuery> {
        let mut old_queries = vec![];
        for future in self.dice_times.iter_mut().chain(self.bin_times.iter_mut())
                                                .chain(self.raster_times.iter_mut())
                                                .chain(self.other_times.iter_mut()) {
            if let Some(old_query) = future.poll(device) {
                old_queries.push(old_query)
            }
        }
        old_queries
    }

    pub(crate) fn total_time(&self) -> Option<RenderTime> {
        let dice_time = total_time_of_timer_futures(&self.dice_times);
        let bin_time = total_time_of_timer_futures(&self.bin_times);
        let raster_time = total_time_of_timer_futures(&self.raster_times);
        let other_time = total_time_of_timer_futures(&self.other_times);
        match (dice_time, bin_time, raster_time, other_time) {
            (Some(dice_time), Some(bin_time), Some(raster_time), Some(other_time)) => {
                Some(RenderTime { dice_time, bin_time, raster_time, other_time })
            }
            _ => None,
        }
    }
}

impl<D> TimerFuture<D> where D: Device {
    pub(crate) fn new(query: D::TimerQuery) -> TimerFuture<D> {
        TimerFuture::Pending(query)
    }

    fn poll(&mut self, device: &D) -> Option<D::TimerQuery> {
        let duration = match *self {
            TimerFuture::Pending(ref query) => device.try_recv_timer_query(query),
            TimerFuture::Resolved(_) => None,
        };
        match duration {
            None => None,
            Some(duration) => {
                match mem::replace(self, TimerFuture::Resolved(duration)) {
                    TimerFuture::Resolved(_) => unreachable!(),
                    TimerFuture::Pending(old_query) => Some(old_query),
                }
            }
        }
    }
}

fn total_time_of_timer_futures<D>(futures: &[TimerFuture<D>]) -> Option<Duration> where D: Device {
    let mut total = Duration::default();
    for future in futures {
        match *future {
            TimerFuture::Pending(_) => return None,
            TimerFuture::Resolved(time) => total += time,
        }
    }
    Some(total)
}

#[derive(Clone, Copy, Debug)]
pub struct RenderTime {
    pub dice_time: Duration,
    pub bin_time: Duration,
    pub raster_time: Duration,
    pub other_time: Duration,
}

impl RenderTime {
    #[inline]
    pub fn total_time(&self) -> Duration {
        self.dice_time + self.bin_time + self.raster_time + self.other_time
    }
}

impl Default for RenderTime {
    #[inline]
    fn default() -> RenderTime {
        RenderTime {
            dice_time: Duration::new(0, 0),
            bin_time: Duration::new(0, 0),
            raster_time: Duration::new(0, 0),
            other_time: Duration::new(0, 0),
        }
    }
}

impl Add<RenderTime> for RenderTime {
    type Output = RenderTime;

    #[inline]
    fn add(self, other: RenderTime) -> RenderTime {
        RenderTime {
            dice_time: self.dice_time + other.dice_time,
            bin_time: self.bin_time + other.bin_time,
            raster_time: self.raster_time + other.raster_time,
            other_time: self.other_time + other.other_time,
        }
    }
}

impl Div<usize> for RenderTime {
    type Output = RenderTime;

    #[inline]
    fn div(self, divisor: usize) -> RenderTime {
        let divisor = divisor as u32;
        RenderTime {
            dice_time: self.dice_time / divisor,
            bin_time: self.bin_time / divisor,
            raster_time: self.raster_time / divisor,
            other_time: self.other_time / divisor,
        }
    }
}
