use super::super::instruction::*;
use super::StmtResult;
use crate::parser::ast::*;

impl super::Lowering {
    // ========== If/Else ==========

    pub(super) fn lower_if(
        &mut self,
        condition: &Expr,
        then_block: &Block,
        else_block: Option<&Block>,
    ) -> Value {
        let cond_val = self.lower_expr(condition);
        let cond_regions = self.take_pending_regions();
        let cond_bb = self.current_block_label;

        let bb_then = self.fresh_block();
        let bb_merge = self.fresh_block();

        match else_block {
            Some(else_blk) => self.lower_if_else(
                cond_val,
                cond_regions,
                cond_bb,
                bb_then,
                bb_merge,
                then_block,
                else_blk,
            ),
            None => self.lower_if_only(
                cond_val,
                cond_regions,
                cond_bb,
                bb_then,
                bb_merge,
                then_block,
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_if_else(
        &mut self,
        cond_val: Value,
        cond_regions: Vec<CfgRegion>,
        cond_bb: u32,
        bb_then: u32,
        bb_merge: u32,
        then_block: &Block,
        else_blk: &Block,
    ) -> Value {
        let bb_else = self.fresh_block();

        self.seal_block(Terminator::CondBr {
            cond: cond_val,
            then_bb: bb_then,
            else_bb: bb_else,
        });

        // then block
        self.start_block(bb_then, vec![]);
        self.push_scope();
        let (then_result, then_inner_regions) = self.lower_block_stmts(then_block);
        self.pop_scope();
        let mut then_region = then_inner_regions;
        // Don't seal yet -- need to know merge type first

        // else block
        let then_block_label = self.current_block_label;
        let then_instructions = std::mem::take(&mut self.current_instructions);
        let then_params = std::mem::take(&mut self.current_block_params);

        self.start_block(bb_else, vec![]);
        self.push_scope();
        let (else_result, else_inner_regions) = self.lower_block_stmts(else_blk);
        self.pop_scope();
        let mut else_region = else_inner_regions;

        let else_block_label = self.current_block_label;
        let else_instructions = std::mem::take(&mut self.current_instructions);
        let else_params = std::mem::take(&mut self.current_block_params);

        // Determine if merge block needs a value parameter
        let then_yields = matches!(&then_result, Some(StmtResult::Yield(_)));
        let else_yields = matches!(&else_result, Some(StmtResult::Yield(_)));
        let has_merge_value = then_yields || else_yields;
        // Infer merge type from yield values
        let merge_type = match (&then_result, &else_result) {
            (Some(StmtResult::Yield(v)), _) => {
                self.value_types.get(v).cloned().unwrap_or(BirType::I32)
            }
            (_, Some(StmtResult::Yield(v))) => {
                self.value_types.get(v).cloned().unwrap_or(BirType::I32)
            }
            _ => BirType::I32,
        };

        // Seal then block
        self.current_block_label = then_block_label;
        self.current_instructions = then_instructions;
        self.current_block_params = then_params;
        self.seal_branch_block(&then_result, bb_merge, &merge_type);
        then_region.push(CfgRegion::Block(bb_then));

        // Seal else block
        self.current_block_label = else_block_label;
        self.current_instructions = else_instructions;
        self.current_block_params = else_params;
        self.seal_branch_block(&else_result, bb_merge, &merge_type);
        else_region.push(CfgRegion::Block(bb_else));

        // Detect whether both branches diverge (all paths return/break/continue)
        let then_diverges = matches!(
            &then_result,
            Some(StmtResult::Return(_))
                | Some(StmtResult::ReturnVoid)
                | Some(StmtResult::Break)
                | Some(StmtResult::Continue)
        );
        let else_diverges = matches!(
            &else_result,
            Some(StmtResult::Return(_))
                | Some(StmtResult::ReturnVoid)
                | Some(StmtResult::Break)
                | Some(StmtResult::Continue)
        );
        let both_diverge = then_diverges && else_diverges;

        // merge block
        let merge_params = if has_merge_value {
            let result = self.fresh_value();
            self.value_types.insert(result, merge_type.clone());
            self.start_block(bb_merge, vec![(result, merge_type)]);
            result
        } else {
            let result = self.fresh_value(); // dummy for Unit
            self.start_block(bb_merge, vec![]);
            if both_diverge {
                // merge block is unreachable; signal caller to seal it
                self.last_expr_diverged = true;
            }
            result
        };

        self.pending_regions.push(CfgRegion::IfElse {
            cond_region: cond_regions,
            cond_bb,
            cond_value: cond_val,
            then_region,
            else_region,
            merge_bb: bb_merge,
        });

        merge_params
    }

    /// Seal a then/else branch block based on its StmtResult.
    fn seal_branch_block(
        &mut self,
        result: &Option<StmtResult>,
        merge_bb: u32,
        merge_type: &BirType,
    ) {
        match result {
            Some(StmtResult::Yield(v)) => {
                self.seal_block(Terminator::Br {
                    target: merge_bb,
                    args: vec![(*v, merge_type.clone())],
                });
            }
            Some(StmtResult::Return(v)) => {
                self.seal_block(Terminator::Return(*v));
            }
            Some(StmtResult::ReturnVoid) => {
                self.seal_block(Terminator::ReturnVoid);
            }
            Some(StmtResult::Break) | Some(StmtResult::Continue) => {
                self.seal_block(Terminator::ReturnVoid);
            }
            _ => {
                self.seal_block(Terminator::Br {
                    target: merge_bb,
                    args: vec![],
                });
            }
        }
    }

    fn lower_if_only(
        &mut self,
        cond_val: Value,
        cond_regions: Vec<CfgRegion>,
        cond_bb: u32,
        bb_then: u32,
        bb_merge: u32,
        then_block: &Block,
    ) -> Value {
        // if without else
        self.seal_block(Terminator::CondBr {
            cond: cond_val,
            then_bb: bb_then,
            else_bb: bb_merge,
        });

        self.start_block(bb_then, vec![]);
        self.push_scope();
        let (then_result, then_inner_regions) = self.lower_block_stmts(then_block);
        self.pop_scope();

        let mut then_region = then_inner_regions;

        match then_result {
            Some(StmtResult::Return(v)) => {
                self.seal_block(Terminator::Return(v));
            }
            Some(StmtResult::ReturnVoid) => {
                self.seal_block(Terminator::ReturnVoid);
            }
            Some(StmtResult::Break) | Some(StmtResult::Continue) => {
                self.seal_block(Terminator::ReturnVoid);
            }
            _ => {
                self.seal_block(Terminator::Br {
                    target: bb_merge,
                    args: vec![],
                });
            }
        }
        then_region.push(CfgRegion::Block(bb_then));

        self.start_block(bb_merge, vec![]);

        self.pending_regions.push(CfgRegion::IfOnly {
            cond_region: cond_regions,
            cond_bb,
            cond_value: cond_val,
            then_region,
            merge_bb: bb_merge,
        });

        // If without else returns Unit -- return dummy value
        let result = self.fresh_value();
        self.emit(Instruction::Literal {
            result,
            value: 0,
            ty: BirType::Unit,
        });
        result
    }

    // ========== While ==========

    pub(super) fn lower_while(
        &mut self,
        condition: &Expr,
        body: &Block,
        nobreak: Option<&Block>,
    ) -> Value {
        let entry_bb = self.current_block_label;

        // Collect mutable var values for block args
        let mutable_vars = self.collect_mutable_var_values();

        let bb_header = self.fresh_block();
        let bb_body = self.fresh_block();
        let bb_exit = self.fresh_block();

        // Seal entry block with Br to header
        let entry_args: Vec<(Value, BirType)> = mutable_vars
            .iter()
            .map(|(_, val, ty)| (*val, ty.clone()))
            .collect();
        self.seal_block(Terminator::Br {
            target: bb_header,
            args: entry_args,
        });

        // Header block: receives mutable vars as params
        let header_params: Vec<(Value, BirType)> = mutable_vars
            .iter()
            .map(|(_, _, ty)| {
                let v = self.fresh_value();
                self.value_types.insert(v, ty.clone());
                (v, ty.clone())
            })
            .collect();

        self.start_block(bb_header, header_params.clone());

        // Remap mutable vars to header params
        for (i, (name, _, _)) in mutable_vars.iter().enumerate() {
            let param_val = header_params[i].0;
            self.assign_var(name, param_val);
        }

        // Push loop context
        self.loop_stack.push(super::LoopContext {
            header_bb: bb_header,
            exit_bb: bb_exit,
            break_ty: None,
        });

        // Evaluate condition
        let cond_val = self.lower_expr(condition);
        let header_regions = self.take_pending_regions();

        // Determine nobreak target
        let bb_nobreak = if nobreak.is_some() {
            self.fresh_block()
        } else {
            bb_exit
        };

        self.seal_block(Terminator::CondBr {
            cond: cond_val,
            then_bb: bb_body,
            else_bb: bb_nobreak,
        });

        // Body block
        self.start_block(bb_body, vec![]);
        self.push_scope();
        let (body_result, body_inner_regions) = self.lower_block_stmts(body);
        self.pop_scope();

        let has_inner_regions = !body_inner_regions.is_empty();
        let mut body_region = body_inner_regions;

        // Track the last block in the body (may differ from bb_body if if/while regions exist)
        let last_body_bb = self.current_block_label;

        // Only emit back-edge if body didn't diverge (break/continue/return)
        match body_result {
            Some(StmtResult::Break) | Some(StmtResult::Continue) => {
                // Already sealed by lower_stmt with BrBreak/BrContinue
                // Seal the dummy block that lower_stmt started
                self.seal_block(Terminator::ReturnVoid);
            }
            Some(StmtResult::Return(v)) => {
                self.seal_block(Terminator::Return(v));
            }
            Some(StmtResult::ReturnVoid) => {
                self.seal_block(Terminator::ReturnVoid);
            }
            _ => {
                // Normal fall-through: emit back-edge to header
                let updated_args: Vec<(Value, BirType)> = mutable_vars
                    .iter()
                    .map(|(name, _, ty)| (self.lookup_var(name), ty.clone()))
                    .collect();
                self.seal_block(Terminator::Br {
                    target: bb_header,
                    args: updated_args,
                });
            }
        }

        // Add body blocks to body_region:
        // - If body_inner_regions is empty, bb_body IS last_body_bb (no if/while inside)
        // - If body_inner_regions is non-empty, bb_body is already used as cond_bb
        //   inside the first region (e.g., IfOnly), so we add last_body_bb instead
        let body_diverged = matches!(
            body_result,
            Some(StmtResult::Break)
                | Some(StmtResult::Continue)
                | Some(StmtResult::Return(_))
                | Some(StmtResult::ReturnVoid)
        );
        if has_inner_regions {
            // bb_body is used as cond_bb inside the first inner region (IfOnly etc.)
            // Add last_body_bb only if body didn't diverge (otherwise it's an unreachable dummy)
            if !body_diverged {
                body_region.push(CfgRegion::Block(last_body_bb));
            }
        } else {
            // No inner regions -- bb_body contains all body instructions + terminator
            body_region.push(CfgRegion::Block(bb_body));
        }

        // Pop loop context and get break type
        let loop_ctx = self.loop_stack.pop().unwrap();
        let break_ty = loop_ctx.break_ty;

        // Restore mutable var scope to header params.
        // The body may have reassigned vars (e.g., `a = b` makes a point to b's value).
        // After the loop, mutable vars should map to their header block parameters,
        // which hold the correct values at loop exit.
        for (i, (name, _, _)) in mutable_vars.iter().enumerate() {
            let param_val = header_params[i].0;
            self.assign_var(name, param_val);
        }

        // Nobreak block
        let nobreak_region = if let Some(nobreak_blk) = nobreak {
            self.lower_nobreak_block(nobreak_blk, bb_nobreak, bb_exit, &break_ty)
        } else {
            vec![]
        };

        // Exit block
        let while_result = if let Some(bty) = break_ty {
            let result_val = self.fresh_value();
            self.start_block(bb_exit, vec![(result_val, bty.clone())]);
            self.value_types.insert(result_val, bty);
            result_val
        } else {
            self.start_block(bb_exit, vec![]);
            let result = self.fresh_value();
            self.emit(Instruction::Literal {
                result,
                value: 0,
                ty: BirType::Unit,
            });
            self.value_types.insert(result, BirType::Unit);
            result
        };

        self.pending_regions.push(CfgRegion::While {
            entry_bb,
            header_region: header_regions,
            header_bb: bb_header,
            cond_value: cond_val,
            body_region,
            nobreak_region,
            exit_bb: bb_exit,
        });

        while_result
    }

    fn lower_nobreak_block(
        &mut self,
        nobreak_blk: &Block,
        bb_nobreak: u32,
        bb_exit: u32,
        break_ty: &Option<BirType>,
    ) -> Vec<CfgRegion> {
        self.start_block(bb_nobreak, vec![]);
        self.push_scope();
        let (nobreak_result, nobreak_inner_regions) = self.lower_block_stmts(nobreak_blk);
        self.pop_scope();

        let mut nb_region = nobreak_inner_regions;

        // Seal nobreak block -> Br to exit_bb with yield value
        match nobreak_result {
            Some(StmtResult::Yield(v)) => {
                if break_ty.is_some() {
                    let ty = self.value_types.get(&v).cloned().unwrap_or(BirType::I32);
                    self.seal_block(Terminator::Br {
                        target: bb_exit,
                        args: vec![(v, ty)],
                    });
                } else {
                    self.seal_block(Terminator::Br {
                        target: bb_exit,
                        args: vec![],
                    });
                }
            }
            Some(StmtResult::Return(v)) => {
                self.seal_block(Terminator::Return(v));
            }
            Some(StmtResult::ReturnVoid) => {
                self.seal_block(Terminator::ReturnVoid);
            }
            _ => {
                self.seal_block(Terminator::Br {
                    target: bb_exit,
                    args: vec![],
                });
            }
        }

        nb_region.push(CfgRegion::Block(bb_nobreak));
        nb_region
    }
}
