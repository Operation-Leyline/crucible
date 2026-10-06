//! The service's own middleware, one file each, used as `middlewares::FailTimes`.

mod fail_times;

pub use fail_times::FailTimes;
