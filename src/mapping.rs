use crate::{Error, Result};

pub fn scale(value: u64, numerator: u32, denominator: u32, ceil: bool) -> Result<u64> {
    if numerator == 0 || denominator == 0 {
        return Err(Error::InvalidConfig("zero sample rate".into()));
    }
    let product = u128::from(value) * u128::from(numerator);
    let result = if ceil {
        product.div_ceil(u128::from(denominator))
    } else {
        product / u128::from(denominator)
    };
    u64::try_from(result).map_err(|_| Error::Constraint("frame coordinate overflow".into()))
}
