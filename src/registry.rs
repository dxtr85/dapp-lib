use super::content::ContentID;

// ChangeRegistry is a listing of ContentIDs sorted by their update time,
// most recently updated first, that is not being kept inside Datastore.
// By taking first n (let's say 100) most recently updated ContentIDs and sending them
// over to a Neighbor in the very first message we exchange with him it will make
// synchronization faster.
pub struct ChangeRegistry([ContentID; 100]);
impl ChangeRegistry {
    pub fn new() -> Self {
        ChangeRegistry([0; 100])
    }
    pub fn insert(&mut self, content_id: ContentID) {
        let mut prev = std::mem::replace(&mut self.0[0], content_id);
        for curr in self.0.iter_mut().skip(1) {
            if prev == content_id || prev == *curr {
                // println!("Insert complete: {:?}", temp.0);
                break;
            }
            prev = std::mem::replace(curr, prev);
        }
    }

    pub fn read(&self) -> Vec<ContentID> {
        let mut result = Vec::with_capacity(100);
        let mut prev = self.0[0];
        result.push(prev);
        for curr in self.0.iter().skip(1) {
            if prev == *curr {
                // println!("Insert complete: {:?}", temp.0);
                break;
            }
            result.push(*curr);
            prev = *curr;
        }
        result
    }
}
