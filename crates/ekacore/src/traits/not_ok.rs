mod private {
    pub trait Sealed {}
}

pub trait NotOk<E>: private::Sealed {
    fn not_ok(self) -> Result<(), E>;
}

impl<E> private::Sealed for Option<E> {}

impl<E> NotOk<E> for Option<E> {
    fn not_ok(self) -> Result<(), E> {
        match self {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
}
