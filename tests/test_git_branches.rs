use jenkins::utils::get_current_branch;
use jenkins::utils::get_git_branches;

#[test]
fn main() {
    let branches = get_git_branches();
    println!("branches: {:?}", branches);
    let current_branch = get_current_branch();
    println!("current_branch: {:?}", current_branch);
}
