pub fn cycle_next<T>(items: &[T], current: usize) -> usize {
    if current == items.len() - 1 {
        0
    }
    else {
        current + 1
    }
}

pub fn cycle_previous<T>(items: &[T], current: usize) -> usize {
    if current == 0 {
        items.len() - 1
    }
    else {
        current - 1
    }
}
