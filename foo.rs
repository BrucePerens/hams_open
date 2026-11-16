fn outer(x: i32) -> i32 {
    fn inner(y: i32) -> i32 {
        if x > 0 {
        x
    } else {
        -x
    }
    }
    if x > 0 {
        inner(x)
    } else {
        inner(-x)
    }
}
