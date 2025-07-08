pub trait Discard {
    fn discard(self);
}

impl<T: Sized> Discard for T {
    fn discard(self) -> () {}
}
