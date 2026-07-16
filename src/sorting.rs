use std::cmp::Ordering;

use crate::model::{FileEntry, FolderEntry, SortDirection, SortField};

pub fn sort_folders(folders: &mut [FolderEntry], field: SortField, direction: SortDirection) {
    folders.sort_by(|left, right| {
        apply_direction(compare_folders(left, right, field), direction)
            .then_with(|| natord::compare(&left.name, &right.name))
    });
}

pub fn sort_files(files: &mut [FileEntry], field: SortField, direction: SortDirection) {
    files.sort_by(|left, right| {
        apply_direction(compare_files(left, right, field), direction)
            .then_with(|| natord::compare(&left.name, &right.name))
    });
}

fn compare_folders(left: &FolderEntry, right: &FolderEntry, field: SortField) -> Ordering {
    match field {
        SortField::Name | SortField::Type => natord::compare(&left.name, &right.name),
        SortField::Modified => left.modified_unix.cmp(&right.modified_unix),
        SortField::Size => left.size_bytes.cmp(&right.size_bytes),
    }
}

fn compare_files(left: &FileEntry, right: &FileEntry, field: SortField) -> Ordering {
    match field {
        SortField::Name => natord::compare(&left.name, &right.name),
        SortField::Modified => left.modified_unix.cmp(&right.modified_unix),
        SortField::Size => left.size_bytes.cmp(&right.size_bytes),
        SortField::Type => natord::compare(&left.extension, &right.extension),
    }
}

fn apply_direction(ordering: Ordering, direction: SortDirection) -> Ordering {
    match direction {
        SortDirection::Ascending => ordering,
        SortDirection::Descending => ordering.reverse(),
    }
}

#[cfg(test)]
mod tests {
    use super::{sort_files, sort_folders};
    use crate::model::{FileEntry, FolderEntry, ItemId, SortDirection, SortField};
    use std::path::PathBuf;

    fn file(name: &str, size: u64, modified: u64, extension: &str) -> FileEntry {
        let path = PathBuf::from(name);
        FileEntry {
            id: ItemId::from(path.clone()),
            path,
            name: name.into(),
            is_image: extension == "jpg",
            extension: extension.into(),
            size_bytes: size,
            modified_unix: modified,
        }
    }

    fn folder(name: &str, size: u64) -> FolderEntry {
        let path = PathBuf::from(name);
        FolderEntry {
            id: ItemId::from(path.clone()),
            path,
            name: name.into(),
            first_image: None,
            image_count: 0,
            total_files: 0,
            size_bytes: size,
            modified_unix: 0,
        }
    }

    #[test]
    fn natural_name_sort_places_product_2_before_product_10() {
        let mut files = vec![
            file("Product 10.jpg", 10, 10, "jpg"),
            file("Product 2.jpg", 20, 20, "jpg"),
        ];

        sort_files(&mut files, SortField::Name, SortDirection::Ascending);

        assert_eq!(files[0].name, "Product 2.jpg");
        assert_eq!(files[1].name, "Product 10.jpg");
    }

    #[test]
    fn descending_size_sort_is_applied_to_folders() {
        let mut folders = vec![folder("Small", 10), folder("Large", 100)];

        sort_folders(&mut folders, SortField::Size, SortDirection::Descending);

        assert_eq!(folders[0].name, "Large");
    }
}
