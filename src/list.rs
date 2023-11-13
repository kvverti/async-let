use crate::{wait::DriveWaitFor, ReadyOrNot};
use core::{future::Future, marker::PhantomData};

/// Represents a typed list of no background futures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Empty {
    pub(crate) _priv: (),
}

/// Represents a typed list of one or more background futures.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct At<'fut, F: Future, Tail> {
    pub(crate) node: ReadyOrNot<'fut, F>,
    pub(crate) tail: Tail,
    // needed to tell derive macros that this type indirectly contains F::Output
    pub(crate) _holds_output: PhantomData<F::Output>,
}

pub trait FutList: DriveWaitFor {}

impl FutList for Empty {}

impl<F: Future, T: FutList> FutList for At<'_, F, T> {}

pub struct Z(());
pub struct S<I>(I);

pub trait Detach<'fut, F: Future, I> {
    type Output;

    fn detach(self) -> (ReadyOrNot<'fut, F>, Self::Output);
}

impl<'fut, F: Future, T> Detach<'fut, F, Z> for At<'fut, F, T> {
    type Output = T;

    fn detach(self) -> (ReadyOrNot<'fut, F>, Self::Output) {
        (self.node, self.tail)
    }
}

impl<'fut, F: Future, I, H: Future, T> Detach<'fut, F, S<I>> for At<'fut, H, T>
where
    T: Detach<'fut, F, I>,
{
    type Output = At<'fut, H, T::Output>;

    fn detach(self) -> (ReadyOrNot<'fut, F>, Self::Output) {
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
