/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

registry.category("web_tour.tours").add('ham_signup_tour', {
    url: '/web/signup',
    steps: () => [
        {
            content: "Ensure Ham is selected",
            trigger: "input#type_ham",
            run: "click",
        },
        {
            content: "Fill in Email",
            trigger: "input[name='login']",
            run: "edit ham_tour_user@example.com",
        },
        {
            content: "Fill in Password",
            trigger: "input[name='password']",
            run: "edit testpassword",
        },
        {
            content: "Confirm Password",
            trigger: "input[name='confirm_password']",
            run: "edit testpassword",
        },
        {
            content: "Fill in Callsign",
            trigger: "input[name='callsign']",
            run: "edit W1AWTOUR",
        },
        {
            content: "Select CAPTCHA Answer",
            trigger: "input.ham-captcha-radio",
            run: "click",
        },
        {
            content: "Click away to blur input",
            trigger: "body",
            run: "click"
        },
        {
            content: "Submit Signup",
            trigger: "button[type='submit']",
            run: "click",
            expectUnloadPage: true
        }
    ]
});

registry.category("web_tour.tours").add('swl_signup_tour', {
    url: '/web/signup',
    steps: () => [
        {
            content: "Select SWL Type",
            trigger: "input#type_swl",
            run: "click",
        },
        {
            content: "Wait for SWL fields to be visible",
            trigger: "#swl_fields:not(.d-none)",
            run: function() {}
        },
        {
            content: "Fill in Email",
            trigger: "input[name='login']",
            run: "edit swl_tour_user@example.com",
        },
        {
            content: "Fill in Password",
            trigger: "input[name='password']",
            run: "edit testpassword",
        },
        {
            content: "Confirm Password",
            trigger: "input[name='confirm_password']",
            run: "edit testpassword",
        },
        {
            content: "Fill in First Name",
            trigger: "input[name='real_first_name']",
            run: "edit SWLFirst",
        },
        {
            content: "Fill in Last Name",
            trigger: "input[name='real_last_name']",
            run: "edit SWLLast",
        },
        {
            content: "Fill in Zip",
            trigger: "input[name='zip_code']",
            run: "edit 12345",
        },
        {
            content: "Fill in Desired Username",
            trigger: "input[name='swl_handle']",
            run: "edit swluser",
        },
        {
            content: "Click away to blur input",
            trigger: "body",
            run: "click"
        },
        {
            content: "Submit SWL Signup",
            trigger: "button[type='submit']",
            run: "click",
            expectUnloadPage: true
        }
    ]
});
