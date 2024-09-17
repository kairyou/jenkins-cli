use jenkins::utils::get_current_branch;
use jenkins::utils::get_git_branches;

#[test]
fn test_git_branches() {
    let branches = get_git_branches();
    println!("branches: {:?}", branches);
}

#[test]
fn test_current_branch() {
    let current_branch = get_current_branch();
    println!("current_branch: {:?}", current_branch);
}
