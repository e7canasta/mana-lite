pub fn contains_idr(data: &[u8]) -> bool {
    let len = data.len();
    let mut i = 0;
    let mut nal_count = 0;
    while i + 3 < len && nal_count < 12 {
        if data[i] == 0x00 && data[i + 1] == 0x00 {
            if data[i + 2] == 0x01 {
                let nal = data[i + 3] & 0x1F;
                if nal == 5 {
                    return true;
                }
                i += 4;
                nal_count += 1;
            } else if i + 4 < len && data[i + 2] == 0x00 && data[i + 3] == 0x01 {
                let nal = data[i + 4] & 0x1F;
                if nal == 5 {
                    return true;
                }
                i += 5;
                nal_count += 1;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_idr_detects_type_5() {
        let data = [0x00, 0x00, 0x00, 0x01, 0x65, 0x88, 0x84, 0x00];
        assert!(contains_idr(&data));
    }

    #[test]
    fn contains_idr_rejects_type_1() {
        let data = [0x00, 0x00, 0x00, 0x01, 0x41, 0x9a, 0x00, 0x00];
        assert!(!contains_idr(&data));
    }

    #[test]
    fn contains_idr_3byte_start_code() {
        let data = [0x00, 0x00, 0x01, 0x65, 0x88];
        assert!(contains_idr(&data));
    }

    #[test]
    fn contains_idr_empty_and_short() {
        assert!(!contains_idr(&[]));
        assert!(!contains_idr(&[0x00, 0x00, 0x00]));
    }

    #[test]
    fn contains_idr_misses_avcc_length_prefixed() {
        let data = [0x00, 0x00, 0x00, 0x04, 0x65, 0x88, 0x84, 0x00];
        assert!(!contains_idr(&data));
    }
}
