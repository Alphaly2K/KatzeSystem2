// 原分区会移动同级项；KCS 随后的查询可见该顺序。
pub(super) fn sort(ids: &mut [usize], key: impl Fn(usize) -> (bool, u32)) {
    sort_with_limit(ids, ids.len(), &key);
}

pub(super) fn candidate_count(ids: &[usize], state: impl Fn(usize) -> (u32, u32)) -> usize {
    let Some(&first) = ids.first() else {
        return 0;
    };
    let (_, drawn) = state(first);
    let mut previous = u32::MAX;
    ids.iter()
        .take_while(|&&id| {
            let (weight, flag) = state(id);
            let eligible = weight <= previous && flag == drawn;
            previous = weight;
            eligible
        })
        .count()
}

fn sort_with_limit(mut ids: &mut [usize], mut limit: usize, key: &impl Fn(usize) -> (bool, u32)) {
    while ids.len() > 32 {
        if limit == 0 {
            heap_sort(ids, key);
            return;
        }
        let (left, right) = partition(ids, key);
        limit = limit / 2 + limit / 4;
        let (low, high) = ids.split_at_mut(right);
        let low = &mut low[..left];
        if low.len() < high.len() {
            sort_with_limit(low, limit, key);
            ids = high;
        } else {
            sort_with_limit(high, limit, key);
            ids = low;
        }
    }
    for i in 1..ids.len() {
        let value = ids[i];
        let mut j = i;
        while j > 0 && key(value) < key(ids[j - 1]) {
            ids[j] = ids[j - 1];
            j -= 1;
        }
        ids[j] = value;
    }
}

fn median(ids: &mut [usize], a: usize, b: usize, c: usize, key: &impl Fn(usize) -> (bool, u32)) {
    if key(ids[b]) < key(ids[a]) {
        ids.swap(a, b);
    }
    if key(ids[c]) < key(ids[b]) {
        ids.swap(b, c);
        if key(ids[b]) < key(ids[a]) {
            ids.swap(a, b);
        }
    }
}

fn partition(ids: &mut [usize], key: &impl Fn(usize) -> (bool, u32)) -> (usize, usize) {
    let n = ids.len();
    let middle = n / 2;
    if n > 41 {
        let step = n / 8;
        median(ids, 0, step, 2 * step, key);
        median(ids, middle - step, middle, middle + step, key);
        median(ids, n - 1 - 2 * step, n - 1 - step, n - 1, key);
        median(ids, step, middle, n - 1 - step, key);
    } else {
        median(ids, 0, middle, n - 1, key);
    }
    let pivot = key(ids[middle]);
    let mut first = middle;
    let mut last = middle + 1;
    while first > 0 && key(ids[first - 1]) == pivot {
        first -= 1;
    }
    while last < n && key(ids[last]) == pivot {
        last += 1;
    }
    let (mut left, mut right) = (first, last);
    loop {
        while right < n {
            match key(ids[right]).cmp(&pivot) {
                std::cmp::Ordering::Less => break,
                std::cmp::Ordering::Equal => {
                    ids.swap(last, right);
                    last += 1;
                }
                std::cmp::Ordering::Greater => {}
            }
            right += 1;
        }
        while left > 0 {
            match key(ids[left - 1]).cmp(&pivot) {
                std::cmp::Ordering::Greater => break,
                std::cmp::Ordering::Equal => {
                    first -= 1;
                    ids.swap(first, left - 1);
                }
                std::cmp::Ordering::Less => {}
            }
            left -= 1;
        }
        match (left == 0, right == n) {
            (true, true) => return (first, last),
            (true, false) => {
                if last != right {
                    ids.swap(first, last);
                }
                ids.swap(first, right);
                first += 1;
                last += 1;
                right += 1;
            }
            (false, true) => {
                left -= 1;
                first -= 1;
                ids.swap(left, first);
                last -= 1;
                ids.swap(first, last);
            }
            (false, false) => {
                left -= 1;
                ids.swap(left, right);
                right += 1;
            }
        }
    }
}

fn heap_sort(ids: &mut [usize], key: &impl Fn(usize) -> (bool, u32)) {
    fn place(ids: &mut [usize], root: usize, value: usize, key: &impl Fn(usize) -> (bool, u32)) {
        let mut hole = root;
        while hole * 2 + 1 < ids.len() {
            let mut child = hole * 2 + 1;
            if child + 1 < ids.len() && key(ids[child + 1]) >= key(ids[child]) {
                child += 1;
            }
            ids[hole] = ids[child];
            hole = child;
        }
        while hole > root {
            let parent = (hole - 1) / 2;
            if key(ids[parent]) >= key(value) {
                break;
            }
            ids[hole] = ids[parent];
            hole = parent;
        }
        ids[hole] = value;
    }
    for i in (0..ids.len() / 2).rev() {
        place(ids, i, ids[i], key);
    }
    for i in (1..ids.len()).rev() {
        let value = ids[i];
        ids[i] = ids[0];
        place(&mut ids[..i], 0, value, key);
    }
}

#[cfg(test)]
mod tests {
    use super::sort;

    #[test]
    fn long_partition_preserves_observed_tie_order() {
        let weights = [
            6, 5, 1, 5, 3, 1, 1, 2, 4, 3, 5, 3, 2, 5, 0, 5, 0, 3, 2, 5, 5, 2, 2, 1, 2, 0, 4, 2, 6,
            6, 5, 1, 2, 1, 0, 0, 6, 4, 4, 0, 0, 3,
        ];
        let mut ids: Vec<_> = (0..weights.len()).collect();
        sort(&mut ids, |id| (false, weights[id]));
        assert_eq!(
            ids,
            [
                25, 14, 16, 34, 35, 39, 40, 5, 33, 2, 6, 31, 23, 32, 22, 21, 7, 18, 27, 12, 24, 41,
                4, 9, 11, 17, 26, 8, 37, 38, 30, 3, 1, 20, 19, 10, 15, 13, 0, 28, 29, 36,
            ]
        );
    }
}
