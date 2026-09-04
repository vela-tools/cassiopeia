use strum::Display;

/// The store operation that was being attempted when a backing-store error arose.
///
/// Carried in the error so a message can name what failed rather than only where, and named as a
/// value so two stores cannot describe the same operation in two different phrasings.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
#[strum(serialize_all = "lowercase")]
pub enum StoreAction {
    /// Opening the on-disk database.
    #[strum(serialize = "open the database")]
    OpenDatabase,
    /// Beginning a transaction.
    #[strum(serialize = "begin a transaction")]
    BeginTransaction,
    /// Committing a transaction.
    #[strum(serialize = "commit a transaction")]
    CommitTransaction,
    /// Creating a table.
    #[strum(serialize = "create a table")]
    CreateTable,
    /// Opening a table inside a transaction.
    #[strum(serialize = "open a table")]
    OpenTable,
    /// Inserting a row.
    #[strum(serialize = "insert a row")]
    Insert,
    /// Reading a row.
    #[strum(serialize = "read a row")]
    Read,
    /// Iterating a table.
    #[strum(serialize = "iterate a table")]
    Iterate,
    /// Removing a row.
    #[strum(serialize = "remove a row")]
    Remove,
    /// Setting a transaction's durability.
    #[strum(serialize = "set transaction durability")]
    SetDurability,
    /// Deleting a table.
    #[strum(serialize = "delete a table")]
    DeleteTable,
}

#[cfg(test)]
mod tests {
    use crate::store_action::StoreAction;

    #[test]
    fn an_action_reads_as_the_operation_it_names() {
        assert_eq!(StoreAction::OpenDatabase.to_string(), "open the database");
        assert_eq!(StoreAction::CommitTransaction.to_string(), "commit a transaction");
    }
}
