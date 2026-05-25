use recon_domain::UserRole;
use crate::error::AuthError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission { ViewRecon, AssignBreak, ProposeResolution, ApproveResolution, ManageUsers, ManageData }

pub fn permitted(role: UserRole, perm: Permission) -> bool {
    use Permission::*;
    use UserRole::*;
    match perm {
        ViewRecon | AssignBreak | ProposeResolution | ManageData => true,
        ApproveResolution => matches!(role, Approver | Admin),
        ManageUsers => matches!(role, Admin),
    }
}

pub fn require(role: UserRole, perm: Permission) -> Result<(), AuthError> {
    if permitted(role, perm) { Ok(()) } else { Err(AuthError::Forbidden) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use recon_domain::UserRole::*;
    #[test]
    fn approve_requires_approver_or_admin() {
        assert!(!permitted(Operator, Permission::ApproveResolution));
        assert!(permitted(Approver, Permission::ApproveResolution));
        assert!(permitted(Admin, Permission::ApproveResolution));
    }
    #[test]
    fn manage_users_admin_only() {
        assert!(!permitted(Operator, Permission::ManageUsers));
        assert!(!permitted(Approver, Permission::ManageUsers));
        assert!(permitted(Admin, Permission::ManageUsers));
    }
    #[test]
    fn view_and_assign_open_to_all() {
        for r in [Operator, Approver, Admin] {
            assert!(permitted(r, Permission::ViewRecon));
            assert!(permitted(r, Permission::AssignBreak));
            assert!(permitted(r, Permission::ProposeResolution));
        }
    }
    #[test]
    fn require_maps_to_forbidden() {
        assert_eq!(require(Operator, Permission::ManageUsers), Err(AuthError::Forbidden));
        assert_eq!(require(Admin, Permission::ManageUsers), Ok(()));
    }
    #[test]
    fn manage_data_open_to_all_roles() {
        for r in [Operator, Approver, Admin] {
            assert!(permitted(r, Permission::ManageData));
        }
    }
}
