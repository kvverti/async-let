use crate::{wait::DriveWaitFor, ReadyOrNot};
use core::{future::Future, marker::PhantomData};

/// Represents a typed list of no background futures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Empty {
    pub(crate) _priv: (),
}

/// Represents a typed list of one or more background futures.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct At<F: Future, Tail>
{
    pub(crate) node: ReadyOrNot<F>,
    pub(crate) tail: Tail,
    // needed to tell derive macros that this type indirectly contains F::Output
    pub(crate) _holds_output: PhantomData<F::Output>,
}

pub trait FutList: DriveWaitFor {}

impl FutList for Empty {}

impl<F: Future + Unpin, T: FutList> FutList for At<F, T>
{
}

pub struct Z(());
pub struct S<I>(I);

pub trait Detach<F: Future, I>
{
    type Output;

    fn detach(self) -> (ReadyOrNot<F>, Self::Output);
}

impl<F: Future, T> Detach<F, Z> for At<F, T>
{
    type Output = T;

    fn detach(self) -> (ReadyOrNot<F>, Self::Output) {
        (self.node, self.tail)
    }
}

impl<F: Future, I, H: Future, T> Detach<F, S<I>> for At<H, T>
where
    T: Detach<F, I>,
{
    type Output = At<H, T::Output>;

    fn detach(self) -> (ReadyOrNot<F>, Self::Output) {
        let (val, tail) = self.tail.detach();
        (
            val,
            At {
                node: self.node,
                tail,
                _holds_output: PhantomData,
            },
        )
    }
}
