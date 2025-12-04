#![no_std]
use soroban_sdk::{Address, Env, String, Vec, contract, contracterror, contractevent, contractimpl, contracttype, panic_with_error};

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Wishes(Address),
    NextId(Address),
    ChristmasDeadline,
    Admin,
    NaughtyList
}

#[derive(Clone)]
#[contracttype]
pub struct Wish {
    pub id: u32,
    pub text: String,
    pub created_at_ledger: u32,
    pub fulfilled: bool,
}

#[contractevent]
pub struct WishAddedEvent {
    pub user: Address,
    pub add: u32,
}

#[contractevent]
pub struct WishFulfilledEvent {
    pub user: Address,
    pub wish_id: u32,
}

#[contracterror]
pub enum ContractError {
  WishNotFound = 1,
  TooLateToChange = 2,
  YouAreNaughty = 3
}

#[contract]
pub struct SeasonalWishlist;

// Helpers to manage TTL (Time To Live)
// We bump the lifespan of data every time it is accessed.
fn bump_persistent_ttl(env: &Env, key: &DataKey) {
    env.storage().persistent().extend_ttl(key, 2_000, 5_000); // If < 2000 ledgers, bump to 5000
}

fn bump_instance_ttl(env: &Env) {
    env.storage().instance().extend_ttl(2_000, 5_000);
}

fn fail(env: &Env, e: ContractError) -> ! {
  panic_with_error!(env, e);
}

fn ensure_not_christmas(env: &Env) {
    // Get the current time from the blockchain ledger
    let current_time = env.ledger().timestamp();
    // The deadline timestamp (Unix seconds).
    // Example: Dec 25, 2025 00:00:00 UTC
    let christmas_deadline = env.storage().instance().get::<_, u64>(&DataKey::ChristmasDeadline).unwrap_or(1_766_620_800);
    if current_time >= christmas_deadline {
        // It is Christmas (or later), we cannot accept changes!
        fail(&env, ContractError::TooLateToChange);
    }
}

fn check_naughty_list(env: &Env, user: &Address) {
    // Fetch the list. If it doesn't exist, default to an empty list (everyone is nice!)
    let naughty_list: Vec<Address> = env.storage().instance()
        .get(&DataKey::NaughtyList)
        .unwrap_or(Vec::new(env));

    // The "Check it Twice" logic
    if naughty_list.contains(user) {
        // Stop execution immediately
        fail(env, ContractError::YouAreNaughty);
    }
}
#[contractimpl]
impl SeasonalWishlist {
    pub fn __constructor(env: Env, admin: Address, christmas_deadline: u64, naughty_list: Vec<Address>) {
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::ChristmasDeadline, &christmas_deadline);
        env.storage().instance().set(&DataKey::NaughtyList, &naughty_list);
    }

    pub fn set_christmas_deadline(env: &Env, christmas_deadline: u64) {
      let admin: Address = env.storage().instance().get(&DataKey::Admin).expect("Santa missing");
      admin.require_auth();
      
      env.storage().instance().set(&DataKey::ChristmasDeadline, &christmas_deadline);
    }

    /// Kid's function: Add a wish.
    pub fn add_wish(env: Env, user: Address, text: String) -> u32 {
        ensure_not_christmas(&env);
        // AUTH: Ensure the transaction signer is actually the user
        user.require_auth();

        check_naughty_list(&env, &user);

        // 1. Generate ID
        let id_key = DataKey::NextId(user.clone());
        let mut next_id: u32 = env.storage().persistent().get(&id_key).unwrap_or(1);
        let current_id = next_id;
        next_id += 1;
        env.storage().persistent().set(&id_key, &next_id);

        // 2. Load existing wishes
        let wish_key = DataKey::Wishes(user.clone());
        let mut wishes: Vec<Wish> = env.storage().persistent().get(&wish_key).unwrap_or_else(|| Vec::new(&env));

        // 3. Add new wish
        wishes.push_back(Wish {
            id: current_id,
            text,
            created_at_ledger: env.ledger().sequence(),
            fulfilled: false, 
        });

        // 4. Save and Bump TTL
        env.storage().persistent().set(&wish_key, &wishes);
        bump_persistent_ttl(&env, &wish_key);
        bump_instance_ttl(&env);

        // EVENT: Emit an event so indexers know a wish was added
        WishAddedEvent {
          user,
          add: current_id
        }.publish(&env);

        current_id
    }

    /// Santa's function: Mark a wish as fulfilled.
    pub fn mark_fulfilled(env: Env, user: Address, wish_id: u32) {
        // AUTH: Get the admin address and require THEIR signature
        let admin: Address = env.storage().instance().get(&DataKey::Admin).expect("Santa missing");
        admin.require_auth();

        let wish_key = DataKey::Wishes(user.clone());
        let mut wishes: Vec<Wish> = env.storage().persistent().get(&wish_key).unwrap_or_else(|| Vec::new(&env));

        // Iterate to find the wish and update it
        let mut found = false;
        for i in 0..wishes.len() {
            let mut wish = wishes.get(i).unwrap();
            if wish.id == wish_id {
                wish.fulfilled = true;
                wishes.set(i, wish);
                found = true;
                break;
            }
        }
        
        if !found { fail(&env, ContractError::WishNotFound); }

        env.storage().persistent().set(&wish_key, &wishes);
        bump_persistent_ttl(&env, &wish_key);
        
        WishFulfilledEvent {
          user,
          wish_id
        }.publish(&env);
    }
    
    /// View function to see a user's list
    pub fn get_list(env: Env, user: Address) -> Vec<Wish> {
        let key = DataKey::Wishes(user.clone());
        let wishes = env.storage().persistent().get(&key).unwrap_or_else(|| Vec::new(&env));
        // Even reading data requires bumping TTL to keep it alive!
        bump_persistent_ttl(&env, &key); 
        wishes
    }
}