/// The index combinations of the non-horizontal dimensions of a grid, one per latitude-longitude
/// plane, in row-major order (the last dimension varies fastest).
///
/// No combination exists when any dimension is empty, and exactly one, the empty combination, when
/// there are no dimensions: a plain latitude-longitude variable is a single plane.
#[derive(Debug, Clone)]
pub struct PlaneIndices {
    sizes: Vec<usize>,
    next: Option<Vec<usize>>,
}

impl PlaneIndices {
    /// The index combinations of dimensions of the given `sizes`.
    #[must_use]
    pub fn new(sizes: Vec<usize>) -> PlaneIndices {
        let next = (!sizes.contains(&0)).then(|| vec![0; sizes.len()]);
        PlaneIndices { sizes, next }
    }
}

impl Iterator for PlaneIndices {
    type Item = Vec<usize>;

    fn next(&mut self) -> Option<Vec<usize>> {
        let current = self.next.take()?;
        // The odometer advances from a copy, because the current combination is handed to the caller.
        let mut following = current.clone();
        for (index, size) in following.iter_mut().zip(&self.sizes).rev() {
            *index += 1;
            if *index < *size {
                self.next = Some(following);
                return Some(current);
            }
            *index = 0;
        }
        Some(current)
    }
}

#[cfg(test)]
mod tests {
    use crate::netcdf::grid::plane_indices::PlaneIndices;
    use pretty_assertions::assert_eq;

    #[test]
    fn combinations_run_in_row_major_order() {
        let combinations: Vec<Vec<usize>> = PlaneIndices::new(vec![2, 3]).collect();
        assert_eq!(combinations, vec![vec![0, 0], vec![0, 1], vec![0, 2], vec![1, 0], vec![1, 1], vec![1, 2]]);
    }

    #[test]
    fn no_dimensions_make_a_single_plane() {
        assert_eq!(PlaneIndices::new(Vec::new()).collect::<Vec<_>>(), vec![Vec::<usize>::new()]);
    }

    #[test]
    fn an_empty_dimension_makes_no_plane() {
        assert_eq!(PlaneIndices::new(vec![2, 0]).count(), 0);
    }
}
