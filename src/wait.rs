use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{
    list::{At, Empty, FutList},
    ReadyOrNot,
};

pub(crate) use private::DriveWaitFor;

pin_project_lite::pin_project! {
    /// Future type for the wait_for method.
    pub struct WaitFor<'group, F, List> {
        #[pin]
        pub(crate) driving_fut: F,
        pub(crate) async_let_group: &'group mut List,
    }
}

impl<F: Future, List: FutList> Future for WaitFor<'_, F, List> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        this.async_let_group.poll_once(cx);
        this.driving_fut.poll(cx)
    }
}

mod private {
    /// Helper trait to poll each async let future when a waited on future is polled.
    pub trait DriveWaitFor {
        fn poll_once(&mut self, cx: &mut super::Context<'_>);
    }
}

impl DriveWaitFor for Empty {
    fn poll_once(&mut self, _cx: &mut Context<'_>) {}
}

impl<F: Future, T: DriveWaitFor> DriveWaitFor for At<'_, F, T> {
    fn poll_once(&mut self, cx: &mut Context<'_>) {
        let At { node, tail } = self;
        if let ReadyOrNot::Not(fut) = node {
            if let Poll::Ready(val) = fut.as_mut().poll(cx) {
                *node = ReadyOrNot::Ready(val);
            }
        }
        tail.poll_once(cx);
    }
}
