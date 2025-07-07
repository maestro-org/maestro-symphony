use crate::{
    serve::error::ServeError,
    storage::{kv_store::Reader, table::Table},
};

pub trait ServeReaderHelper {
    fn get_expected<T>(&self, key: &T::Key) -> Result<T::Value, ServeError>
    where
        T: Table;

    fn get_maybe<T>(&self, key: &T::Key) -> Result<Option<T::Value>, ServeError>
    where
        T: Table;
}

impl ServeReaderHelper for Reader {
    fn get_expected<T>(&self, key: &T::Key) -> Result<T::Value, ServeError>
    where
        T: Table,
    {
        match self.get::<T>(key)? {
            Some(value) => Ok(value),
            None => Err(ServeError::missing_data(key)),
        }
    }

    fn get_maybe<T>(&self, key: &T::Key) -> Result<Option<T::Value>, ServeError>
    where
        T: Table,
    {
        self.get::<T>(key).map_err(|e| e.into())
    }
}
