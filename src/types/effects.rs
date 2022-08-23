use std::rc::Rc;

use crate::cache::{EffectInfoId, ModuleCache};
use crate::error::location::Location;
use crate::types::typechecker::TypeBindings;
use crate::types::Type;
use crate::util::fmap;

use super::typechecker::{self, OccursResult, UnificationBindings};
use super::{TypeBinding, TypeVariableId};

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct EffectSet {
    pub effects: Vec<Effect>,
    pub replacement: TypeVariableId,
}

pub type Effect = (EffectInfoId, Vec<Rc<Type>>);

impl EffectSet {
    /// Create a new polymorphic effect set
    pub fn any(cache: &mut ModuleCache) -> EffectSet {
        EffectSet { effects: vec![], replacement: typechecker::next_type_variable_id(cache) }
    }

    pub fn single(id: EffectInfoId, args: Vec<Rc<Type>>, cache: &mut ModuleCache) -> EffectSet {
        let mut set = EffectSet::any(cache);
        set.effects.push((id, args));
        set
    }

    pub fn new(effects: Vec<(EffectInfoId, Vec<Rc<Type>>)>, cache: &mut ModuleCache) -> EffectSet {
        let mut set = EffectSet::any(cache);
        set.effects = effects;
        set
    }

    pub fn follow_bindings<'a>(&'a self, cache: &'a ModuleCache) -> &'a Self {
        match &cache.type_bindings[self.replacement.0] {
            TypeBinding::Bound(binding) => match binding.as_ref() {
                Type::Effects(effects) => effects.follow_bindings(cache),
                _ => self,
            },
            _ => self,
        }
    }

    pub fn follow_unification_bindings<'a>(
        &'a self, bindings: &'a UnificationBindings, cache: &'a ModuleCache,
    ) -> &'a Self {
        let binding = &cache.type_bindings[self.replacement.0];
        match &binding {
            TypeBinding::Bound(ty) => match ty.as_ref() {
                Type::Effects(effects) => effects.follow_unification_bindings(bindings, cache),
                _ => todo!(),
            },
            _ => match bindings.bindings.get(&self.replacement) {
                Some(ty) => match ty.as_ref() {
                    Type::Effects(effects) => effects.follow_unification_bindings(bindings, cache),
                    _ => self,
                },
                _ => self,
            },
        }
    }

    pub fn replace_all_typevars_with_bindings(
        &self, new_bindings: &mut TypeBindings, cache: &mut ModuleCache,
    ) -> Rc<Type> {
        let ty_binding = &cache.type_bindings[self.replacement.0];

        if let TypeBinding::Bound(bound) = ty_binding {
            if let Type::Effects(effects) = bound.as_ref() {
                return effects.clone().replace_all_typevars_with_bindings(new_bindings, cache);
            }
        }

        let replacement = match new_bindings.get(&self.replacement) {
            Some(ty) => match ty.as_ref() {
                Type::TypeVariable(new_id) => *new_id,
                _ => return ty.clone(),
            },
            None => typechecker::next_type_variable_id(cache),
        };

        let effects = fmap(&self.effects, |(id, args)| {
            (*id, fmap(args, |arg| typechecker::replace_all_typevars_with_bindings(&arg, new_bindings, cache)))
        });

        Rc::new(Type::Effects(EffectSet { effects, replacement }))
    }

    /// Replace any typevars found with the given type bindings
    ///
    /// Compared to `replace_all_typevars_with_bindings`, this function does not instantiate
    /// unbound type variables that were not in type_bindings. Thus if type_bindings is empty,
    /// this function will just clone the original EffectSet.
    pub fn bind_typevars(&self, type_bindings: &TypeBindings, cache: &ModuleCache) -> Rc<Type> {
        // type_bindings is checked for bindings before the cache, see the comment
        // in typechecker::bind_typevar
        let replacement = match type_bindings.get(&self.replacement) {
            Some(ty) => match ty.as_ref() {
                Type::TypeVariable(new_id) => *new_id,
                _ => return ty.clone(),
            },
            None => self.replacement,
        };

        if let TypeBinding::Bound(typ) = &cache.type_bindings[self.replacement.0] {
            return typechecker::bind_typevars(&typ.clone(), type_bindings, cache);
        }

        let effects = fmap(&self.effects, |(id, args)| {
            (*id, fmap(args, |arg| typechecker::bind_typevars(arg, type_bindings, cache)))
        });

        Rc::new(Type::Effects(EffectSet { effects, replacement }))
    }

    pub fn try_unify_with_bindings(
        &self, other: &EffectSet, bindings: &mut UnificationBindings, cache: &mut ModuleCache,
    ) {
        let a = self.follow_unification_bindings(bindings, cache);
        let b = other.follow_unification_bindings(bindings, cache);

        let mut new_effects = a.effects.clone();
        new_effects.append(&mut b.effects.clone());
        new_effects.sort();
        new_effects.dedup();

        let a_id = a.replacement;
        let b_id = b.replacement;

        let new_effect = EffectSet::new(new_effects, cache);
        bindings.bindings.insert(a_id, Rc::new(Type::Effects(new_effect.clone())));
        bindings.bindings.insert(b_id, Rc::new(Type::Effects(new_effect)));
    }

    pub fn combine(&self, other: &EffectSet, cache: &mut ModuleCache) -> EffectSet {
        let a = self.follow_bindings(cache);
        let b = other.follow_bindings(cache);

        let mut new_effects = a.effects.clone();
        new_effects.append(&mut b.effects.clone());
        new_effects.sort();
        new_effects.dedup();

        let a_id = a.replacement;
        let b_id = b.replacement;

        let new_effect = EffectSet::new(new_effects, cache);
        cache.bind(a_id, Rc::new(Type::Effects(new_effect.clone())));
        cache.bind(b_id, Rc::new(Type::Effects(new_effect.clone())));

        new_effect
    }

    pub fn find_all_typevars(&self, polymorphic_only: bool, cache: &ModuleCache) -> Vec<super::TypeVariableId> {
        let this = self.follow_bindings(cache);
        let mut vars = typechecker::find_typevars_in_typevar_binding(this.replacement, polymorphic_only, cache);

        for (_, args) in &this.effects {
            for arg in args {
                vars.append(&mut typechecker::find_all_typevars(arg, polymorphic_only, cache));
            }
        }

        vars
    }

    pub fn contains_any_typevars_from_list(&self, list: &[super::TypeVariableId], cache: &ModuleCache) -> bool {
        let this = self.follow_bindings(cache);
        list.contains(&this.replacement)
            || this
                .effects
                .iter()
                .any(|(_, args)| args.iter().any(|arg| typechecker::contains_any_typevars_from_list(arg, list, cache)))
    }

    pub(super) fn occurs(
        &self, id: super::TypeVariableId, level: super::LetBindingLevel, bindings: &mut UnificationBindings, fuel: u32,
        cache: &mut ModuleCache,
    ) -> OccursResult {
        let this = self.follow_bindings(cache).clone();
        let mut result = typechecker::typevars_match(id, level, this.replacement, bindings, fuel, cache);

        for (_, args) in &this.effects {
            result = result.then_all(args, |arg| typechecker::occurs(id, level, arg, bindings, fuel, cache));
        }
        result
    }

    /// The dual of 'combine', where combine takes two EffectSets and returns their union,
    /// this function will return the difference between self and other.
    pub(super) fn handle_effects_from(&self, other: EffectSet, cache: &mut ModuleCache) {
        let a = self.follow_bindings(cache).clone();
        let b = other.follow_bindings(cache).clone();

        let mut new_effects = Vec::with_capacity(a.effects.len());

        for a_effect in a.effects.iter() {
            match find_matching_effect(a_effect, &b.effects, cache) {
                Ok(bindings) => bindings.perform(cache),
                Err(()) => new_effects.push(a_effect.clone()),
            }
        }

        let a_id = a.replacement;

        let new_effect = EffectSet::new(new_effects, cache);
        cache.bind(a_id, Rc::new(Type::Effects(new_effect)));
    }
}

fn find_matching_effect(effect: &Effect, set: &[Effect], cache: &mut ModuleCache) -> Result<UnificationBindings, ()> {
    let (effect_id, effect_args) = effect;
    for (other_id, other_args) in set {
        if effect_id == other_id {
            let bindings = UnificationBindings::empty();
            let no_loc = Location::builtin();
            let no_error = "";

            match typechecker::try_unify_all_with_bindings(effect_args, other_args, bindings, no_loc, cache, no_error) {
                Ok(bindings) => return Ok(bindings),
                Err(_) => (),
            }
        }
    }
    Err(())
}
