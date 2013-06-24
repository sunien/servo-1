/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use layout::box::{RenderBox};
use layout::context::LayoutContext;
use layout::display_list_builder::{DisplayListBuilder, ExtraDisplayListData};
use layout::display_list_builder::{FlowDisplayListBuilderMethods};
use layout::flow::{FloatFlow, FlowData};
use layout::model::{MaybeAuto};
use layout::float_context::{FloatContext, PlacementInfo, FloatLeft};

use core::cell::Cell;
use geom::point::Point2D;
use geom::rect::Rect;
use gfx::display_list::DisplayList;
use gfx::geometry::Au;
use gfx::geometry;
use servo_util::tree::{TreeNodeRef, TreeUtils};

pub struct FloatFlowData {
    /// Data common to all flows.
    common: FlowData,

    /// The associated render box.
    box: Option<RenderBox>,

    containing_width: Au,


    /// Index into the box list for inline floats
    index: Option<uint>,

}

impl FloatFlowData {
    pub fn new(common: FlowData) -> FloatFlowData {
        FloatFlowData {
            common: common,
            containing_width: Au(0),
            box: None,
            index: None,
        }
    }

    pub fn teardown(&mut self) {
        self.common.teardown();
        for self.box.each |box| {
            box.teardown();
        }
        self.box = None;
        self.index = None;
    }
}

impl FloatFlowData {
    pub fn bubble_widths_float(@mut self, ctx: &LayoutContext) {
        let mut min_width = Au(0);
        let mut pref_width = Au(0);

        self.common.num_floats = 1;

        for FloatFlow(self).each_child |child_ctx| {
            //assert!(child_ctx.starts_block_flow() || child_ctx.starts_inline_flow());

            do child_ctx.with_mut_base |child_node| {
                min_width = geometry::max(min_width, child_node.min_width);
                pref_width = geometry::max(pref_width, child_node.pref_width);
                child_node.floats_in = FloatContext::new(child_node.num_floats);
            }
        }

        self.box.map(|&box| {
            let style = box.style();
            do box.with_model |model| {
                model.compute_borders(style)
            }

            min_width = min_width.add(&box.get_min_width(ctx));
            pref_width = pref_width.add(&box.get_pref_width(ctx));
        });

        self.common.min_width = min_width;
        self.common.pref_width = pref_width;
    }

    pub fn assign_widths_float(@mut self, _: &LayoutContext) { 
        debug!("assign_widths_float: assigning width for flow %?",  self.common.id);
        // position.size.width is set by parent even though we don't know
        // position.origin yet.
        let mut remaining_width = self.common.position.size.width;
        self.containing_width = remaining_width;
        let mut x_offset = Au(0);

        for self.box.each |&box| {
            let style = box.style();
            do box.with_model |model| {
                // Can compute padding here since we know containing block width.
                model.compute_padding(style, remaining_width);

                // Margins for floats are 0 if auto.
                let margin_top = MaybeAuto::from_margin(style.margin_top(),
                                                        remaining_width).spec_or_default(Au(0));
                let margin_bottom = MaybeAuto::from_margin(style.margin_bottom(),
                                                           remaining_width).spec_or_default(Au(0));
                let margin_left = MaybeAuto::from_margin(style.margin_left(),
                                                        remaining_width).spec_or_default(Au(0));
                let margin_right = MaybeAuto::from_margin(style.margin_right(),
                                                           remaining_width).spec_or_default(Au(0));



                let shrink_to_fit = geometry::min(self.common.pref_width, 
                                                  geometry::max(self.common.min_width, 
                                                                remaining_width));


                let width = MaybeAuto::from_width(style.width(), 
                                                  remaining_width).spec_or_default(shrink_to_fit);
                debug!("assign_widths_float -- width: %?", width);

                model.margin.top = margin_top;
                model.margin.right = margin_right;
                model.margin.bottom = margin_bottom;
                model.margin.left = margin_left;

                x_offset = model.offset();
                remaining_width = width;
            }

            do box.with_mut_base |base| {
                //The associated box is the border box of this flow
                base.position.origin.x = base.model.margin.left;

                let pb = base.model.padding.left + base.model.padding.right +
                    base.model.border.left + base.model.border.right;
                base.position.size.width = remaining_width + pb;
            }
        }

        self.common.position.size.width = remaining_width;

        for FloatFlow(self).each_child |kid| {
            //assert!(kid.starts_block_flow() || kid.starts_inline_flow());

            do kid.with_mut_base |child_node| {
                child_node.position.origin.x = x_offset;
                child_node.position.size.width = remaining_width;
            }
        }
    }

    pub fn assign_height_float(@mut self, ctx: &mut LayoutContext) {
        for FloatFlow(self).each_child |kid| {
            kid.assign_height(ctx);
        }

        let mut cur_y = Au(0);
        let mut top_offset = Au(0);

        for self.box.each |&box| {
            do box.with_model |model| {
                top_offset = model.margin.top + model.border.top + model.padding.top;
                cur_y += top_offset;
            }
        }

        for FloatFlow(self).each_child |kid| {
            do kid.with_mut_base |child_node| {
                child_node.position.origin.y = cur_y;
                cur_y += child_node.position.size.height;
            }
        }

        let mut height = cur_y - top_offset;
        
        let mut noncontent_height = Au(0);
        self.box.map(|&box| {
            do box.with_mut_base |base| {
                //The associated box is the border box of this flow
                base.position.origin.y = base.model.margin.top;

                noncontent_height = base.model.padding.top + base.model.padding.bottom +
                    base.model.border.top + base.model.border.bottom;
                base.position.size.height = height + noncontent_height;

                noncontent_height += base.model.margin.top + base.model.margin.bottom;
            }
        });

        
        //TODO(eatkinson): compute heights properly using the 'height' property.
        for self.box.each |&box| {

            let height_prop = 
                MaybeAuto::from_height(box.style().height(), Au(0)).spec_or_default(Au(0));

            height = geometry::max(height, height_prop) + noncontent_height;
            debug!("assign_height_float -- height: %?", height);
            do box.with_mut_base |base| {
                base.position.size.height = height;
            }
        }

        let info = PlacementInfo {
            width: self.common.position.size.width,
            height: height,
            ceiling: Au(0),
            max_width: self.containing_width,
            f_type: FloatLeft,
        };

        self.common.floats_out = self.common.floats_in.add_float(&info);



    }

    pub fn build_display_list_float<E:ExtraDisplayListData>(@mut self,
                                                            builder: &DisplayListBuilder,
                                                            dirty: &Rect<Au>, 
                                                            offset: &Point2D<Au>,
                                                            list: &Cell<DisplayList<E>>) {
        self.box.map(|&box| {
            box.build_display_list(builder, dirty, offset, list)
        });


        // go deeper into the flow tree
        let flow = FloatFlow(self);
        for flow.each_child |child| {
            flow.build_display_list_for_child(builder, child, dirty, offset, list)
        }
    }
}

