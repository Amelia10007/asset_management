use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrSuger<E>(E);

impl<E> ErrSuger<E> {
    pub fn err_from(e: E) -> Result<(), ErrSuger<E>>
    where
        ErrSuger<E>: From<E>,
    {
        Err(Self::from(e))
    }
}

impl<E> Display for ErrSuger<E>
where
    E: Display,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<E> Error for ErrSuger<E> where ErrSuger<E>: Debug + Display {}

impl<E> From<E> for ErrSuger<E> {
    fn from(e: E) -> Self {
        Self(e)
    }
}

pub trait OkOpt {
    type Ok;

    fn ok_opt<E>(self, err: E) -> Result<Self::Ok, ErrSuger<E>>;

    fn ok_opt_once<E, F>(self, f: F) -> Result<Self::Ok, ErrSuger<E>>
    where
        F: FnOnce() -> E;
}

impl<T> OkOpt for Option<T> {
    type Ok = T;

    fn ok_opt<E>(self, msg: E) -> Result<Self::Ok, ErrSuger<E>> {
        match self {
            Some(t) => Ok(t),
            None => Err(ErrSuger(msg)),
        }
    }

    fn ok_opt_once<E, F>(self, f: F) -> Result<Self::Ok, ErrSuger<E>>
    where
        F: FnOnce() -> E,
    {
        match self {
            Some(t) => Ok(t),
            None => Err(ErrSuger(f())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OkOpt;

    #[test]
    fn test_ok() {
        let opt = Some(42);
        let res = opt.ok_opt("oops");

        assert_eq!(Ok(42), res);
    }

    #[test]
    fn test_err() {
        let opt: Option<i32> = None;
        let res = opt.ok_opt(String::from("oops"));

        assert!(res.is_err());
    }
}
