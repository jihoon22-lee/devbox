//! SQL text enum conversion shared by persisted domain enums.
#[macro_export]
macro_rules! sql_text_enum {
    ($name:ident { $($variant:ident = $text:literal),+ $(,)? }) => {
        impl ::rusqlite::types::ToSql for $name {
            fn to_sql(&self) -> ::rusqlite::Result<::rusqlite::types::ToSqlOutput<'_>> {
                Ok(::rusqlite::types::ToSqlOutput::from(match self { $(Self::$variant => $text),+ }))
            }
        }
        impl ::rusqlite::types::FromSql for $name {
            fn column_result(value: ::rusqlite::types::ValueRef<'_>) -> ::rusqlite::types::FromSqlResult<Self> {
                match value.as_str()? { $($text => Ok(Self::$variant),)+ _ => Err(::rusqlite::types::FromSqlError::InvalidType) }
            }
        }
    };
}
