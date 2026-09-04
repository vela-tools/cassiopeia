/// Whether a batch of work runs across threads or one item at a time.
///
/// Every batch-processing stage (expansion, resolution, extraction, transformation) shares this
/// one toggle so the choice between a Rayon parallel iterator and sequential processing is named
/// the same way everywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Parallelism {
    /// Process the batch concurrently with a Rayon parallel iterator.
    #[default]
    Parallel,
    /// Process the batch one item at a time on the calling thread.
    Sequential,
}

#[cfg(test)]
mod tests {
    use crate::parallelism::Parallelism;

    #[test]
    fn parallel_is_the_default() {
        assert_eq!(Parallelism::default(), Parallelism::Parallel);
    }
}
